mod backfiller;
mod healer;
mod ingest_journal;
mod mv_matcher;
mod raw_healer;
mod remediator;
mod streamer;
mod types;

pub mod nodestream {
    tonic::include_proto!("nodestream");
}

use std::env;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 && args[1] == "--backfill" {
        // View re-derive from existing world_logs ONLY — not raw cores.
        let start_block: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let end_block: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(32700000);
        let chunk_size: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(10000);

        info!("=== TANK-ENGINE HISTORICAL BACKFILL MODE (views from world_logs) ===");
        backfiller::run_historical_backfill(start_block, end_block, chunk_size)
            .await
            .map_err(|e| -> BoxError { e.to_string().into() })?;
    } else if args.len() >= 2 && args[1] == "--remediate" {
        let start_block: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(29000000);
        let end_block: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30067000);
        let tranche_size: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1000);

        info!("=== TANK-ENGINE DATA REMEDIATOR MODE ===");
        remediator::run_remediator_loop(start_block, end_block, tranche_size)
            .await
            .map_err(|e| -> BoxError { e.to_string().into() })?;
    } else if args.len() >= 2 && args[1] == "--raw-heal" {
        // Phase-2 F4: RPC → world_blocks/txs/logs/traces (Go HealBlock port).
        let start_block: u64 = args
            .get(2)
            .and_then(|s| s.parse().ok())
            .ok_or("--raw-heal requires <start> <end>")?;
        let end_block: u64 = args
            .get(3)
            .and_then(|s| s.parse().ok())
            .ok_or("--raw-heal requires <start> <end>")?;
        if start_block > end_block {
            return Err("--raw-heal start must be <= end".into());
        }
        raw_healer::run_raw_heal_range(start_block, end_block).await?;
    } else if args.len() >= 2 && args[1] == "--ingest-journal" {
        // C1 journal of the live engine: `status` is read-only; `resolve` needs tank-live stopped (writer lock).
        const USAGE: &str = "usage: --ingest-journal status | --ingest-journal resolve <block> <table|all> done|retry";
        let dir = Path::new(ingest_journal::JOURNAL_DIR);
        match args.get(2).map(String::as_str) {
            Some("status") => print!(
                "{}",
                ingest_journal::status_report(dir, Path::new(ingest_journal::CUTOVER_FILE))?
            ),
            Some("resolve") => {
                let block: u64 = args.get(3).and_then(|s| s.parse().ok()).ok_or(USAGE)?;
                let table = args.get(4).ok_or(USAGE)?;
                let how = match args.get(5).map(String::as_str) {
                    Some("done") => ingest_journal::Resolution::Done,
                    Some("retry") => ingest_journal::Resolution::Retry,
                    _ => return Err(USAGE.into()),
                };
                let lock = Path::new(ingest_journal::WRITER_LOCK);
                print!(
                    "{}",
                    streamer::resolve_journal(dir, lock, block, table, how).await?
                );
            }
            _ => return Err(USAGE.into()),
        }
    } else {
        println!("=== TANK-ENGINE RUST UNIFIED PIPELINE (STREAMER + HEALER) ===");
        // C1 v2: no writer runs without the durable journal, its writer lock and the cutover floor; the streamer and
        // the healer share them (and the per-block locks).
        let ingest = Arc::new(open_ingest().await);
        let healer_ingest = Arc::clone(&ingest);
        tokio::spawn(async move {
            healer::run_healer_loop(healer_ingest).await;
        });
        streamer::run_live_streamer(ingest)
            .await
            .map_err(|e| -> BoxError { e.to_string().into() })?;
    }

    Ok(())
}

/// The journal (writer lock held) and the cutover floor, retried with backoff until both are available.
async fn open_ingest() -> ingest_journal::Ingest {
    let dir = Path::new(ingest_journal::JOURNAL_DIR);
    let lock = Path::new(ingest_journal::WRITER_LOCK);
    let cutover = Path::new(ingest_journal::CUTOVER_FILE);
    let mut backoff_secs = 1u64;
    loop {
        match ingest_journal::Journal::open(dir, lock) {
            Ok(journal) => match ingest_journal::Cutover::load(cutover) {
                Ok(c) => {
                    info!(
                        "TANK-ENGINE ingest journal ready dir={} writer_lock={} cutover_floor={} complete_tip={} windows_since_ms={}",
                        dir.display(),
                        lock.display(),
                        c.floor,
                        c.complete_tip,
                        c.created_at_ms
                    );
                    return ingest_journal::Ingest::new(journal, cutover.to_path_buf(), &c);
                }
                Err(e) => error!(
                    "TANK-ENGINE INGEST_CUTOVER_UNREADABLE {}: {} — not writing without the C1 v2 cutover floor (deploy pack install writes it); retry in {}s",
                    cutover.display(),
                    e,
                    backoff_secs
                ),
            },
            Err(e) => error!(
                "TANK-ENGINE INGEST_JOURNAL_UNAVAILABLE dir={} lock={}: {} — not writing without the durable ingest journal (C1); retry in {}s",
                dir.display(),
                lock.display(),
                e,
                backoff_secs
            ),
        }
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}
