use clickhouse::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::ingest_journal::{EntryKind, Ingest};
use crate::raw_healer;
use crate::streamer;

const CLICKHOUSE_URL: &str = "http://127.0.0.1:8123";
const CLICKHOUSE_DB: &str = "worldchain";

/// Named Row for SELECT block_number — bare u64 Row fetch always deserializes empty.
#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct GapBn {
    block_number: u64,
}

/// One row: how many world_blocks rows exist for a height, and their transactions_count.
#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockTxCount {
    n: u64,
    txs: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct Count {
    n: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TransactionIndex {
    tx_index: u32,
}

/// The healer shares the ingest journal and block locks with the streamer. Every tick resumes JSON-only Heal
/// owners through raw RPC before the queue drains, including when their original queue row resolved or sequence
/// draining is disabled. All core sends and owner releases use durable terminal fences.
pub async fn run_healer_loop(ingest: Arc<Ingest>) {
    info!("TANK-ENGINE Healer background engine online (gap claims + trace + sequence/raw + gap derive).");
    let ch_client = Client::default()
        .with_url(CLICKHOUSE_URL)
        .with_database(CLICKHOUSE_DB);

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_default();

    if let Err(e) = raw_healer::ensure_sequence_gaps_table(&ch_client).await {
        warn!("TANK-ENGINE ensure world_sequence_gaps: {}", e);
    }
    let target = streamer::ClickHouseTarget::new(ch_client.clone());
    let mut deriver = crate::backfiller::GapDeriver::new(ingest.floor);

    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;

        if let Err(e) = streamer::check_schema(&ch_client).await {
            error!("TANK-ENGINE Healer: INGEST_SCHEMA_NOT_READY {} — nothing healed or derived this tick", e);
            continue;
        }

        // Claims first: cheap, and a gap the stream skipped must reach the queue even while healing is slow.
        if let Err(e) = streamer::process_gap_claims(&target, &ingest.journal).await {
            warn!("TANK-ENGINE Healer gap claims warning: {}", e);
        }

        if let Err(e) = resume_derive_entries(&target, &ingest).await {
            warn!("TANK-ENGINE Healer derive resume warning: {}", e);
        }

        if let Err(e) = raw_healer::resume_heal_entries(&ch_client, &http_client, &ingest).await {
            warn!("TANK-ENGINE Healer owner resume warning: {}", e);
        }

        // Prefer traces first so a stuck/slow sequence drain cannot starve heal_trace_gaps
        // (lesson SequenceDrainMegaGapBlocksTraceHealer).
        if let Err(e) = heal_trace_gaps(&ch_client, &http_client, &ingest).await {
            warn!("TANK-ENGINE Healer trace gap run warning: {}", e);
        }

        if raw_healer::env_disable_sequence_drain() {
            info!(
                "TANK-ENGINE Healer: background sequence gap draining disabled (TANK_DISABLE_SEQUENCE_DRAIN)"
            );
        } else if let Err(e) = heal_sequence_gaps(&ch_client, &http_client, &ingest).await {
            warn!("TANK-ENGINE Healer sequence gap run warning: {}", e);
        }

        if let Err(e) = deriver.tick(&ch_client, &target, &ingest).await {
            warn!("TANK-ENGINE Healer gap derive warning: {}", e);
        }
    }
}

/// Derive entries a stop or an error left unfinished: finished first, each under its block's lock.
async fn resume_derive_entries(
    target: &streamer::ClickHouseTarget,
    ingest: &Ingest,
) -> Result<(), String> {
    let loaded = ingest.journal.load(EntryKind::Derive).map_err(|e| {
        format!("ingest journal unreadable ({e}); derive entries left for the operator")
    })?;
    for item in loaded {
        let n = streamer::loaded_block(&item);
        if let Err(e) = streamer::resume_loaded(target, ingest, item).await {
            warn!(
                "TANK-ENGINE Healer: derive entry block={} not finished yet: {}",
                n, e
            );
        }
    }
    Ok(())
}

fn trace_gap_min_block() -> u64 {
    std::env::var("TANK_TRACE_GAP_MIN_BLOCK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

async fn heal_trace_gaps(
    ch: &Client,
    http: &reqwest::Client,
    ingest: &Ingest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let min_bn = trace_gap_min_block();
    // FINAL: ReplacingMergeTree; permanent misses must not pin the ascending queue head.
    let sql = format!(
        "SELECT block_number FROM worldchain.world_trace_gaps FINAL \
         WHERE resolved = 0 \
           AND reason != 'pruned_node_permanent' \
           AND block_number >= {min_bn} \
         ORDER BY block_number ASC LIMIT 100"
    );
    let gaps: Vec<GapBn> = ch.query(&sql).fetch_all().await?;
    for gap in gaps {
        // A parked owner stops only its own height, not later gaps.
        if let Err(e) = heal_trace_gap(ch, http, ingest, gap.block_number).await {
            warn!(
                "TANK-ENGINE Healer: trace gap block={} retained: {}",
                gap.block_number, e
            );
        }
    }
    Ok(())
}

async fn heal_trace_gap(
    ch: &Client,
    http: &reqwest::Client,
    ingest: &Ingest,
    block_num: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _owner = ingest.locks.lock(block_num).await;
    let mut entry = if ingest.journal.owns(block_num) {
        match ingest.journal.entry(block_num)? {
            Some(entry) if entry.kind == EntryKind::Heal => {
                // Only the full raw resume path can certify an existing owner's incomplete table prefix.
                info!(
                    "TANK-ENGINE Healer: trace gap block={} retained for its raw Heal owner resume",
                    block_num
                );
                return Ok(());
            }
            Some(entry) if ingest.journal.entry_payload(&entry)?.is_none() => Some(entry),
            _ => {
                info!("TANK-ENGINE Healer: trace gap block={} is owned by Stream/Derive or an orphan payload; left queued", block_num);
                return Ok(());
            }
        }
    } else {
        None
    };
    if let Some(entry) = entry.as_mut() {
        raw_healer::refresh_core_states(ch, &ingest.journal, entry).await?;
    }
    let stored: Count = ch
        .query(&format!(
            "SELECT toUInt64(count()) AS n FROM worldchain.world_traces WHERE chain_id = 480 AND block_number = {}",
            block_num
        ))
        .fetch_one()
        .await?;
    if stored.n > 0 {
        mark_trace_gap_resolved(ch, block_num, "healer-trace-gap-stored").await?;
        info!(
            "TANK-ENGINE Healer: block={} already holds {} traces; resolved without writing",
            block_num, stored.n
        );
        return Ok(());
    }
    // Missing hash/timestamp must never create rows in partition 197001.
    let (b_hash, block_ts) = match raw_healer::block_hash_and_ts(http, block_num).await? {
        Some(v) => v,
        None => {
            warn!(
                "TANK-ENGINE Healer: block={} has no archive hash/timestamp; trace gap left queued",
                block_num
            );
            return Ok(());
        }
    };
    if let Some(entry) = entry.as_ref() {
        if !entry.block_hash.eq_ignore_ascii_case(b_hash.trim())
            || entry.block_timestamp != block_ts
        {
            return Err(
                format!("block {block_num}: archive identity differs from journal owner").into(),
            );
        }
    }
    let hex_num = format!("0x{:x}", block_num);
    let body: Value = raw_healer::rpc_call(http, "trace_block", json!([hex_num])).await?;
    if let Some(result) = body.get("result").and_then(|r| r.as_array()) {
        let now_ms = entry
            .as_ref()
            .map_or_else(crate::ingest_journal::wall_ms, |e| e.now_ms);
        let traces = raw_healer::trace_rows_from_trace_block(
            result,
            block_num,
            &b_hash,
            block_ts,
            now_ms,
            "healer-trace-gap",
        );
        if !traces.is_empty() {
            let txs: Vec<TransactionIndex> = ch
                .query(&format!(
                    "SELECT tx_index FROM worldchain.world_transactions FINAL WHERE chain_id = 480 AND block_number = {}",
                    block_num
                ))
                .fetch_all()
                .await?;
            if streamer::trace_coverage_missing(
                txs.iter().map(|tx| tx.tx_index),
                |index| traces.iter().any(|trace| trace.tx_index == index),
            ) {
                warn!(
                    "TANK-ENGINE Healer: incomplete trace result for block={}; transaction trace coverage missing; left queued",
                    block_num
                );
                return Ok(());
            }
            let mut entry = match entry {
                Some(entry) => entry,
                None => ingest
                    .journal
                    .create_heal(block_num, &b_hash, block_ts, now_ms)?,
            };
            raw_healer::insert_block_rows(
                ch,
                "world_traces",
                &traces,
                block_num,
                &b_hash,
                &ingest.journal,
                &mut entry,
            )
            .await?;
            raw_healer::complete_heal(&ingest.journal, &mut entry)?;
            mark_trace_gap_resolved(ch, block_num, "healer-trace-gap").await?;
            info!(
                "TANK-ENGINE Healer: successfully healed trace gap for block={}",
                block_num
            );
        } else {
            // Empty traces are complete only for a stored block with zero transactions.
            let tc: BlockTxCount = ch
                .query(&format!(
                    "SELECT toUInt64(count()) AS n, toUInt64(sum(transactions_count)) AS txs FROM worldchain.world_blocks FINAL WHERE chain_id = 480 AND block_number = {}",
                    block_num
                ))
                .fetch_one()
                .await?;
            if tc.n == 0 {
                warn!(
                    "TANK-ENGINE Healer: empty trace result for unknown block={}; left queued",
                    block_num
                );
            } else if tc.txs == 0 {
                mark_trace_gap_resolved(ch, block_num, "healer-trace-gap-no-txs").await?;
            } else {
                warn!("TANK-ENGINE Healer: empty trace result for block={} with {} transactions; left queued", block_num, tc.txs);
            }
        }
    } else if let Some(err) = body.get("error") {
        if trace_miss_is_permanent(&err.to_string()) {
            warn!(
                "TANK-ENGINE Healer: permanent trace miss block={} — marking pruned: {}",
                block_num, err
            );
            mark_trace_gap_resolved(ch, block_num, "pruned_node_permanent").await?;
        } else {
            warn!(
                "TANK-ENGINE Healer: trace RPC error block={}: {}",
                block_num, err
            );
        }
    }
    Ok(())
}

/// Only explicit pruning or missing historical state retires a trace gap.
/// Catch-up and transport failures (including generic -32603 errors) stay queued.
fn trace_miss_is_permanent(err: &str) -> bool {
    let bytes = err.as_bytes();
    let contains = |phrase: &str| {
        bytes.windows(phrase.len()).enumerate().any(|(i, part)| {
            part.eq_ignore_ascii_case(phrase.as_bytes())
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
                && (i + phrase.len() == bytes.len()
                    || !bytes[i + phrase.len()].is_ascii_alphanumeric())
        })
    };
    if [
        "not been executed yet", "not executed yet", "not yet executed",
        "not available yet", "not yet available", "timeout", "timed out",
        "connection", "not pruned", "not been pruned", "never pruned",
        "never been pruned", "not missing historical state",
    ].iter().any(|phrase| contains(phrase)) {
        return false;
    }
    [
        "pruned", "missing historical state", "historical state missing",
        "historical state is missing", "historical state not available",
        "historical state is not available", "historical state unavailable",
        "historical state is unavailable", "historical state not found",
        "historical state is not found",
    ].iter().any(|phrase| contains(phrase))
}

async fn mark_trace_gap_resolved(
    ch: &Client,
    block_num: u64,
    reason: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut update = ch
        .insert("world_trace_gaps")?
        .with_option("async_insert", "0");
    update
        .write(&TraceGapRow {
            block_number: block_num,
            rpc_error_code: 0,
            reason: reason.to_string(),
            attempted_at: now_ms,
            resolved: 1,
            resolved_at: now_ms,
        })
        .await?;
    update.end().await?;
    Ok(())
}

/// Phase-2 F3: drain durable sequence gaps via RPC raw heal (Go HealBlock port), each block under its lock.
async fn heal_sequence_gaps(
    ch: &Client,
    http: &reqwest::Client,
    ingest: &Ingest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Cap per tick to protect RAM/CPU under heavy tip load (Go healer lesson).
    raw_healer::drain_sequence_gaps(ch, http, 25, ingest).await
}

#[derive(Debug, Clone, clickhouse::Row, serde::Serialize)]
pub struct TraceGapRow {
    pub block_number: u64,
    pub rpc_error_code: i32,
    pub reason: String,
    pub attempted_at: u64,
    pub resolved: u8,
    pub resolved_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_bn_is_named_row_not_bare_u64() {
        // Regression: bare u64 Row fetch silently yields empty Vec.
        let g = GapBn {
            block_number: 33_446_134,
        };
        assert_eq!(g.block_number, 33_446_134);
    }

    #[test]
    fn trace_gap_min_block_defaults_zero() {
        std::env::remove_var("TANK_TRACE_GAP_MIN_BLOCK");
        assert_eq!(trace_gap_min_block(), 0);
    }

    #[test]
    fn unexecuted_block_is_not_a_permanent_trace_miss() {
        // The 2026-10-18 outage error, also used by the uncompiled draft patch.
        let err = r#"{"code":-32603,"message":"state at block #36489335 is not available, block has not been executed yet (latest executed: #36489306)"}"#;
        assert!(!trace_miss_is_permanent(err));
    }

    #[test]
    fn transient_trace_misses_stay_queued() {
        for err in [
            r#"{"code":-32603,"message":"internal error"}"#,
            r#"{"code":-32603}"#,
            "block not executed yet",
            "block not yet executed",
            "historical state is not available yet",
            "historical state is not yet available",
            "historical state request timeout",
            "historical state request timed out",
            "connection reset while reading historical state",
            "connection refused by pruned archive",
            "request timeout checking pruned state",
            "historical state is not available, block has not been executed yet",
            "historical state service error",
            "historical transaction not found",
            "pruning is in progress",
            "unpruned state",
            "state is not pruned",
            "state has not been pruned",
            "state is never pruned",
            "state has never been pruned",
            "state is not missing historical state",
            "historical state is not missing",
            "prunedness unknown",
            "missing historical states",
            "",
        ] {
            assert!(!trace_miss_is_permanent(err), "unexpected permanent miss: {err}");
        }
    }

    #[test]
    fn explicit_pruning_is_a_permanent_trace_miss() {
        for err in [
            r#"{"code":-32000,"message":"state at block #100 is pruned"}"#,
            r#"{"code":-32603,"message":"state at block #100 is pruned"}"#,
            "State has been PRUNED",
        ] {
            assert!(trace_miss_is_permanent(err), "expected permanent miss: {err}");
        }
    }

    #[test]
    fn missing_historical_state_is_a_permanent_trace_miss() {
        for err in [
            r#"{"code":-32603,"message":"missing historical state"}"#,
            "historical state missing",
            "historical state is missing",
            "historical state not available",
            "historical state is not available",
            "historical state unavailable",
            "historical state is unavailable",
            "historical state not found",
            "HISTORICAL STATE IS NOT FOUND",
        ] {
            assert!(trace_miss_is_permanent(err), "expected permanent miss: {err}");
        }
    }

    #[tokio::test]
    async fn partial_trace_gap_rpc_result_is_not_inserted_or_resolved() {
        let d = raw_healer::tests::RecoveryDir::new();
        let ingest = d.ingest();
        let mock = raw_healer::tests::RawMock::new(true).await;
        let block_num = 100;
        raw_healer::insert_queue_row(
            &mock.ch,
            "world_trace_gaps",
            &TraceGapRow {
                block_number: block_num,
                rpc_error_code: 0,
                reason: "stream-missing-traces".to_string(),
                attempted_at: 1,
                resolved: 0,
                resolved_at: 0,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            mock.state.lock().unwrap().trace_gap_resolved.get(&block_num),
            Some(&0)
        );
        assert!(!ingest.journal.owns(block_num));

        // RPC returns a nonempty trace array covering tx_index 0, while stored transactions include 0 and 1.
        heal_trace_gap(&mock.ch, &mock.http, &ingest, block_num)
            .await
            .unwrap();

        let state = mock.state.lock().unwrap();
        assert_eq!(state.rpc_methods, ["eth_getBlockByNumber", "trace_block"]);
        assert_eq!(state.trace_inserts, 0);
        assert_eq!(state.trace_gap_heights, vec![block_num]);
        assert_eq!(state.trace_gap_resolved.get(&block_num), Some(&0));
        assert!(!ingest.journal.owns(block_num));
        assert!(ingest.journal.load(EntryKind::Heal).unwrap().is_empty());
    }
}
