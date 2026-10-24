// PATCH: tank-engine in-process reconnect + capped exponential backoff + loud
// gap detection, replacing the old "fatal-exit-on-any-stream-error" behavior.
//
// WHY (root cause, proved 2026-08-10, see TIP-CATCHUP-TRACKER.md incident section):
//   worldchain-exex's collect_block_traces() can block the ExEx notification
//   task for 30-40s+ during catch-up (NonceTooLow-heavy blocks). That starves
//   HTTP/2 keepalive ping/pong on the gRPC connection to tank-engine
//   (http2_keepalive_timeout=5s), the server resets the stream, and
//   `stream.message().await?` returns Err -> old code returned Err from
//   run_live_streamer() -> process exit -> systemd Restart=always (RestartSec=2s)
//   respawns the WHOLE process (CH tip requery, sock-wait poll, tracing re-init,
//   ~15-40s observed). Because subscribe() is broadcast-only from `start_block`,
//   ANY block ExEx emitted while tank-live was down/reconnecting is permanently
//   missed unless something re-requests it (nothing currently does for raw
//   world_blocks/world_transactions/world_traces/world_logs).
//
// WHAT THIS PATCH DOES:
//   1. Wraps connect+subscribe+consume in a supervisor loop inside the process
//      (run_live_streamer), so a stream error causes an in-process reconnect
//      with capped exponential backoff (1s,2s,4s,8s,16s,30s cap) instead of a
//      full process exit. Faster recovery, less log/CH-query churn than
//      systemd bouncing the whole binary every time.
//   2. On every (re)connect, checks whether the first block actually delivered
//      by the stream matches the expected start_block. If it's higher, that is
//      a definite, provable hole (blocks ExEx broadcast while tank was down).
//      This is logged at ERROR with a greppable `INGESTION_GAP_DETECTED` marker
//      naming the exact missing range, instead of silently proceeding as if
//      nothing happened (today's behavior).
//   3. systemd Restart=always stays in place as the outer safety net (e.g. if
//      ClickHouse itself is down and the tip query fails repeatedly) — this
//      patch does not remove it, it just makes the common case (transient gRPC
//      disconnects from ExEx-side stalls) self-heal without a full process
//      bounce, and makes any resulting hole loud instead of silent.
//
// C1 v5 (2026-10-16, traces-epoch incident leftover): durable ownership and fenced per-table sends.
//   Before, the block row went first and every insert ended the block on error: the stream then resumed at
//   max(world_transactions)+1, past a block whose transactions were in, so its logs, traces and every derived table
//   were never written (2026-10-12: 746 blocks), and the blocks skipped by a reconnect were raw-healed but never
//   derived. Now:
//   1. A received block is saved in the durable journal (ingest_journal.rs) before its first insert; a failed save
//      is retried with the same received block (INGEST_JOURNAL_WRITE_RETRY), the stream waits.
//   2. Every insert is synchronous with a shared block/table token and a journaled query ID and client lifetime.
//      Before replay or generation advance every earlier request must be terminal, or its client must have ended
//      and a later server restart (or host reboot) plus flushed process/log absence prove it cannot still arrive.
//      That proof alone never proves noncommit: counted tables count; uncertain unkeyed outputs park but remain resolvable.
//      Proven unsubmitted/pre-pipeline rejected requests retry. Core ACKs are confirmed. A lost payload is rebuilt
//      into its original entry: Done/Parked, generations and send history never reset.
//   3. Stream/derive entries are resumed before new work; parked heights remain owned without blocking later ones.
//      Raw and trace healers, including the CLI, also journal an owner, generation and every send before I/O.
//      Core rows already stored are not written again, and owner removal requires every request fenced.
//   4. A block above the cursor (first block of a connection or inside a stream) is preceded by a durable gap claim
//      saved before the higher block is taken; the healer hands it to world_sequence_gaps in chunks of at most
//      10,000 heights and retries until each chunk is acknowledged. The raw heal fills those blocks and the gap
//      deriver derives them through the same journal (raw_healer.rs), so a gap is never left underived.
//   5. The block row follows transactions, logs and traces; a ledger row follows every table. Its complete manifest
//      includes raw-healed core and zero-row outputs. Owned parked heights do not starve later ready gap blocks.
//   6. The cursor is max(world_transactions tip, newest Stream journal block) + 1; a failed tip query stops the stream.
//      The first stream after the C1 v2 install claims [cutover floor, first block - 1], so blocks the old engine
//      left partly written are completed too.

use clickhouse::Client;
use prost::Message;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixStream;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;
use tracing::{error, info, warn};

use crate::ingest_journal::{
    self, table_class, Cutover, Entry, EntryKind, GapClaim, Ingest, Journal, Loaded, TableClass,
    TableState, TableStatus, LEDGER_TABLE,
};
use crate::mv_matcher::match_and_extract_views;
use crate::nodestream::node_stream_client::NodeStreamClient;
use crate::nodestream::{BlockNotification, SubscribeRequest};
use crate::types::*;

const NODE_SOCK: &str = "/var/run/worldchain/node_stream.sock";
const CLICKHOUSE_URL: &str = "http://127.0.0.1:8123";
const CLICKHOUSE_DB: &str = "worldchain";
const MAX_BACKOFF_SECS: u64 = 30;
/// Client-side limits of one insert (send of a chunk, wait for the server's answer). A send that exceeds them has an
/// unknown outcome; the next attempt reconciles it like any other failed send before anything is sent again.
const INSERT_SEND_TIMEOUT: Duration = Duration::from_secs(60);
const INSERT_END_TIMEOUT: Duration = Duration::from_secs(180);

pub(crate) fn ch_client() -> Client {
    Client::default()
        .with_url(CLICKHOUSE_URL)
        .with_database(CLICKHOUSE_DB)
}

/// Supervisor entry point. Never returns under normal operation: any error from
/// stream_once() is logged and retried in-process with capped exponential
/// backoff. Only truly unrecoverable setup errors (e.g. malformed constants)
/// would propagate, in which case systemd Restart=always remains the backstop.
pub async fn run_live_streamer(ingest: Arc<Ingest>) -> Result<(), Box<dyn std::error::Error>> {
    info!("TANK-ENGINE Live Streamer initializing (in-process reconnect + backoff enabled)...");
    let mut backoff_secs: u64 = 1;

    loop {
        match stream_once(&mut backoff_secs, &ingest).await {
            Ok(()) => unreachable!("stream_once only returns via Err by design"),
            Err(e) => {
                error!(
                    "TANK-ENGINE stream error: {} — reconnecting in {}s (in-process retry, no full restart)",
                    e, backoff_secs
                );
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
}

/// Where the stream stands: the next height it must write, and whether the C1 v2 cutover gap is still to be claimed.
struct StreamPos {
    expected: u64,
    first: bool,
    cutover: Option<Cutover>,
}

/// One connect+subscribe+consume cycle. Resets the caller's backoff counter
/// to 1s as soon as at least one block has been successfully committed, so a
/// long-healthy run doesn't leave a stale large backoff lingering. Always
/// returns Err (never Ok) — either propagated via `?` on a transport failure,
/// or explicitly constructed on clean stream EOF (server-side restart) — so
/// the supervisor loop above always knows to reconnect.
async fn stream_once(
    backoff_secs: &mut u64,
    ingest: &Ingest,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = ch_client();
    let target = ClickHouseTarget::new(client.clone());

    // C1 v2: a re-send is safe only on tables that keep a deduplication window; without it, no stream.
    check_schema(&client).await.map_err(|e| {
        format!(
            "INGEST_SCHEMA_NOT_READY {e}; not streaming (C1 v2 needs the deploy pack schema step)"
        )
    })?;

    // C1: finish every block received earlier (by this process or the one before it) before anything else; an
    // unfinished block stops here and no new block is taken until it is done or parked.
    let journal_max = resume_stream_entries(&target, ingest).await?;

    // C1: fail closed. A failed tip query used to start the stream at block 1 (2026-10-10 and 2026-10-12:
    // INGESTION_GAP_DETECTED [1, tip] and a capped 10,000-block raw-heal enqueue each time).
    let tip = client
        .query("SELECT coalesce((SELECT block_number FROM worldchain.world_transactions WHERE chain_id = 480 ORDER BY block_number DESC LIMIT 1), 0)")
        .fetch_one::<u64>()
        .await
        .map_err(|e| format!("ClickHouse tip query failed ({}); not starting the stream (C1: never from 0)", e))?;
    info!("TANK-ENGINE ClickHouse tip resolved: block={}", tip);

    let start_block = tip.max(journal_max) + 1;
    let cutover = Cutover::load(&ingest.cutover_path).map_err(|e| {
        format!(
            "INGEST_CUTOVER_UNREADABLE {}: {}",
            ingest.cutover_path.display(),
            e
        )
    })?;
    let mut pos = start_position(start_block, cutover);
    // This connection will not ingest below pos.expected. Publish that before the first
    // block arrives: the in-memory cursor dies with the process, and a quiet tip has no
    // newer stream ledger row. A heal-owned height the previous connection already walked
    // past is then visible to the gap deriver once its owner is released. The first
    // connection still publishes the cutover floor, not the transaction tip, so an
    // unclaimed gap is not treated as passed.
    ingest
        .stream_expected
        .fetch_max(pos.expected, std::sync::atomic::Ordering::SeqCst);
    info!(
        "TANK-ENGINE starting forward ingestion stream from block={} (expected={}{})",
        start_block,
        pos.expected,
        if pos.cutover.is_some() {
            ", C1 v2 cutover gap still to claim"
        } else {
            ""
        }
    );

    while !Path::new(NODE_SOCK).exists() {
        warn!("Waiting for Unix socket at {}...", NODE_SOCK);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let channel = Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(|_: Uri| async move {
            Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(
                UnixStream::connect(NODE_SOCK).await?,
            ))
        }))
        .await?;

    let mut gprc_client = NodeStreamClient::new(channel);
    let response = gprc_client
        .subscribe(SubscribeRequest { start_block })
        .await?;
    let mut stream = response.into_inner();

    info!(
        "TANK-ENGINE gRPC stream established at socket {}",
        NODE_SOCK
    );

    while let Some(block) = stream.message().await? {
        if handle_block(&target, ingest, &mut pos, &block).await? {
            // Healthy commit observed — reset backoff so a later, unrelated blip
            // doesn't inherit a stale large delay from a much earlier incident.
            *backoff_secs = 1;
        }
    }

    Err("TANK-ENGINE stream ended (server-side EOF, e.g. ExEx restarted) — reconnecting".into())
}

/// The first stream after the install starts its gap check at the cutover floor (the old engine may have left the
/// floor block partly written and its transactions moved the tip past it); later streams at the cursor.
fn start_position(start_block: u64, cutover: Cutover) -> StreamPos {
    if cutover.claimed_at_ms.is_none() {
        StreamPos {
            expected: cutover.floor.min(start_block),
            first: true,
            cutover: Some(cutover),
        }
    } else {
        StreamPos {
            expected: start_block,
            first: true,
            cutover: None,
        }
    }
}

/// One block from the stream. Ok(true) when the block was finished. Err only when it could not be finished (its
/// journal entry stays; the stream reconnects and resumes it first).
async fn handle_block<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    pos: &mut StreamPos,
    block: &BlockNotification,
) -> Result<bool, String> {
    let number = block.number;
    if number < pos.expected {
        // C1: never derive a block twice (the old loop wrote such a block again).
        report_overlap(target, block, pos.expected).await;
        return Ok(false);
    }
    if number > pos.expected {
        error!(
            "TANK-ENGINE INGESTION_GAP_DETECTED expected_start={} actual_start={} missing_blocks={} missing_range=[{}, {}] — claimed in the journal, enqueued to world_sequence_gaps in chunks by the healer (raw heal + derive)",
            pos.expected, number, number - pos.expected, pos.expected, number - 1
        );
        let source = if pos.cutover.is_some() {
            "c1v2-cutover"
        } else if pos.first {
            "streamer-reconnect"
        } else {
            "streamer-midstream"
        };
        // C1 v2: durable before the higher block is taken, so the cursor never moves past an unclaimed gap.
        persist_gap_claim(&ingest.journal, pos.expected, number - 1, source).await;
    } else if pos.first {
        info!(
            "TANK-ENGINE resumed stream at exact expected height={}, no gap",
            pos.expected
        );
    }
    if let Some(mut c) = pos.cutover.take() {
        c.claimed_at_ms = Some(ingest_journal::wall_ms());
        c.claimed_gap = (number > c.floor).then(|| [c.floor, number - 1]);
        persist_cutover(&ingest.cutover_path, &c).await;
        info!(
            "TANK-ENGINE C1 v2 cutover claimed: floor={} gap={:?}",
            c.floor, c.claimed_gap
        );
    }
    pos.first = false;
    let finished = ingest_new_block(target, ingest, block).await?;
    pos.expected = number + 1;
    // the gap deriver may take heights below this one: each is finished, owned by a journal entry, or claimed
    ingest
        .stream_expected
        .fetch_max(pos.expected, std::sync::atomic::Ordering::SeqCst);
    Ok(finished)
}

async fn persist_gap_claim(journal: &Journal, lo: u64, hi: u64, source: &str) -> GapClaim {
    let detected_at_ms = ingest_journal::wall_ms();
    let mut backoff = 1u64;
    loop {
        match journal.claim_gap(lo, hi, source, detected_at_ms) {
            Ok(c) => return c,
            Err(e) => {
                error!(
                    "TANK-ENGINE INGEST_JOURNAL_WRITE_RETRY gap=[{}, {}]: {} — the block after the gap waits until the gap is saved; retry in {}s",
                    lo, hi, e, backoff
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
}

async fn persist_cutover(path: &Path, c: &Cutover) {
    let mut backoff = 1u64;
    while let Err(e) = c.save(path) {
        error!(
            "TANK-ENGINE INGEST_JOURNAL_WRITE_RETRY cutover {}: {} — retry in {}s",
            path.display(),
            e,
            backoff
        );
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

/// C1: claim a trace-incomplete block as a gap; otherwise journal it before any write. A failed save is retried
/// with the same received block (the stream waits), never dropped.
async fn ingest_new_block<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    block: &BlockNotification,
) -> Result<bool, String> {
    let _owner = ingest.locks.lock(block.number).await;
    if stream_missing_traces(block) {
        persist_gap_claim(&ingest.journal, block.number, block.number, "stream-missing-traces").await;
        warn!(
            "TANK-ENGINE stream block={} missing transaction traces; claimed as sequence gap",
            block.number
        );
        return Ok(false);
    }
    let now_ms = ingest_journal::wall_ms();
    let batches = build_batches(block, now_ms);
    let mut entry = Entry::new(
        EntryKind::Stream,
        block.number,
        &block.hash,
        block.timestamp,
        now_ms,
        &batches.tables(EntryKind::Stream),
    );
    let payload = block.encode_to_vec();
    let mut backoff = 1u64;
    loop {
        match ingest.journal.create(&mut entry, &payload) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Some(owner) = ingest
                    .journal
                    .entry(block.number)
                    .map_err(|e| e.to_string())?
                {
                    if owner.kind == EntryKind::Heal
                        && owner.block_hash.eq_ignore_ascii_case(block.hash.trim())
                    {
                        // Only a received height advances the cursor; a high raw-heal owner does not imply that
                        // every lower gap was claimed. This owner's core and later derivation remain the healer's.
                        return Ok(false);
                    }
                }
                // Existing Stream/Derive ownership is resumed on reconnect, never replaced by a fresh payload.
                return Err(format!(
                    "INGEST_JOURNAL_OWNED block={}: {} — not written again from the stream",
                    block.number, e
                ));
            }
            Err(e) => {
                error!(
                    "TANK-ENGINE INGEST_JOURNAL_WRITE_RETRY block={}: {} — the received block is kept in memory and saved again in {}s; nothing inserted",
                    block.number, e, backoff
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
    Ok(complete_entry(target, ingest, &mut entry, &batches).await? == Completion::Finished)
}

/// C1: finish every unfinished stream entry, oldest first, each under its block's lock. Err (the stream does not
/// start) while one of them cannot be finished yet. Returns the newest block number the journal held.
async fn resume_stream_entries<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
) -> Result<u64, String> {
    let loaded = ingest.journal.load(EntryKind::Stream).map_err(|e| {
        format!(
            "ingest journal unreadable ({}); not streaming until it is fixed by hand",
            e
        )
    })?;
    let mut newest = 0u64;
    for item in loaded {
        newest = newest.max(loaded_block(&item));
        resume_loaded(target, ingest, item).await?;
    }
    Ok(newest)
}

pub(crate) fn loaded_block(item: &Loaded) -> u64 {
    match item {
        Loaded::Entry(e) => e.block_number,
        Loaded::Orphan { block_number, .. } => *block_number,
    }
}

/// Finishes one unfinished journal item (either kind) under its block's lock.
pub(crate) async fn resume_loaded<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    item: Loaded,
) -> Result<(), String> {
    let _owner = ingest.locks.lock(loaded_block(&item)).await;
    match item {
        Loaded::Entry(entry) => resume_owned(target, ingest, entry.block_number).await,
        Loaded::Orphan {
            block_number,
            payload,
        } => {
            let block = BlockNotification::decode(payload.bytes.as_slice()).map_err(|e| {
                format!(
                    "block {}: saved payload does not decode: {}",
                    block_number, e
                )
            })?;
            if block.number != block_number {
                return Err(format!(
                    "block {}: saved payload holds block {}",
                    block_number, block.number
                ));
            }
            let batches = build_batches(&block, payload.now_ms);
            let mut entry = Entry::new(
                payload.kind,
                block_number,
                &block.hash,
                block.timestamp,
                payload.now_ms,
                &batches.tables(payload.kind),
            );
            // no insert runs before an entry is committed: this block has no sends of ours yet
            ingest
                .journal
                .commit_orphan(&mut entry, &payload)
                .map_err(|e| format!("block {}: orphan commit failed: {}", block_number, e))?;
            if entry.kind == EntryKind::Stream && stream_missing_traces(&block) {
                return retire_unsent_stream_to_gap(ingest, &mut entry).await;
            }
            warn!(
                "TANK-ENGINE ingest journal: block={} ({:?}) was saved but not committed before the last stop (no insert ran); writing it now",
                block_number, payload.kind
            );
            complete_entry(target, ingest, &mut entry, &batches)
                .await
                .map(|_| ())
        }
    }
}

/// Resumes the current entry on disk. The caller already holds the block's lock: a scan made before taking it may
/// be stale because the streamer or the gap deriver finished or restored the entry in between.
pub(crate) async fn resume_owned<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    n: u64,
) -> Result<(), String> {
    let entry = ingest
        .journal
        .entry(n)
        .map_err(|e| format!("block {n}: entry read failed: {e}"))?;
    match entry {
        Some(entry) => resume_entry(target, ingest, entry).await,
        None => Ok(()),
    }
}

async fn resume_entry<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    mut entry: Entry,
) -> Result<(), String> {
    if entry.kind == EntryKind::Heal {
        return Ok(()); // the background raw healer resumes this JSON-only core owner
    }
    let n = entry.block_number;
    refresh_decided_sends(target, ingest, &mut entry).await?;
    if entry.is_done() {
        // stopped between the two removals of finish()
        return ingest
            .journal
            .finish(&entry)
            .map_err(|e| format!("block {n}: finish failed: {e}"));
    }
    if !entry.has_pending() {
        return Ok(()); // only parked tables left: the operator's
    }
    let payload = ingest
        .journal
        .entry_payload(&entry)
        .map_err(|e| format!("block {n}: payload read failed: {e}"))?;
    let block = payload
        .and_then(|p| BlockNotification::decode(p.bytes.as_slice()).ok())
        .filter(|b| b.number == n);
    let Some(block) = block else {
        return payload_lost(target, ingest, entry).await;
    };
    if entry.kind == EntryKind::Stream && stream_missing_traces(&block) {
        if stream_entry_is_unsent(&entry) {
            return retire_unsent_stream_to_gap(ingest, &mut entry).await;
        }
        target
            .enqueue_trace_gap(&crate::healer::TraceGapRow {
                block_number: n,
                rpc_error_code: 0,
                reason: "stream-missing-traces".into(),
                attempted_at: ingest_journal::wall_ms(),
                resolved: 0,
                resolved_at: 0,
            })
            .await
            .map_err(|e| format!("block {n}: trace gap enqueue failed: {e}"))?;
        warn!(
            "TANK-ENGINE stream block={} missing transaction traces with send history; queued for the trace-gap healer, resuming the existing Stream owner",
            n
        );
    }
    info!(
        "TANK-ENGINE ingest journal: resuming block={} ({:?})",
        n, entry.kind
    );
    let batches = build_batches(&block, entry.now_ms);
    complete_entry(target, ingest, &mut entry, &batches)
        .await
        .map(|_| ())
}

/// Every send-history field must be empty: advancing a generation can clear sends and current query IDs.
pub(crate) fn stream_entry_is_unsent(entry: &Entry) -> bool {
    entry.tables.iter().all(|t| {
        t.sends == 0
            && t.generation == 0
            && t.first_send_ms.is_none()
            && t.query_ids.is_empty()
            && t.clients.is_empty()
            && t.terminal_query_ids.is_empty()
            && t.rejected_query_ids.is_empty()
    })
}

/// Transfer a legacy Stream payload only when no table was ever sent. Done here is an UNSENT retirement marker,
/// not table or ledger completion. Save it before finish(): a stop after payload removal must not replay the owner.
async fn retire_unsent_stream_to_gap(ingest: &Ingest, entry: &mut Entry) -> Result<(), String> {
    let n = entry.block_number;
    if !stream_entry_is_unsent(entry) {
        return Err(format!("block {n}: Stream retirement requires no send history"));
    }
    persist_gap_claim(&ingest.journal, n, n, "stream-missing-traces").await;
    for t in &mut entry.tables {
        t.status = TableStatus::Done;
        t.note = Some("UNSENT Stream entry retired to sequence gap; no table or ledger completion".into());
    }
    ingest
        .journal
        .save(entry)
        .map_err(|e| format!("block {n}: Stream retirement save failed: {e}; owner kept"))?;
    ingest
        .journal
        .finish(entry)
        .map_err(|e| format!("block {n}: Stream retirement finish failed: {e}"))?;
    warn!(
        "TANK-ENGINE stream block={} missing transaction traces; claimed as sequence gap (retired UNSENT Stream entry)",
        n
    );
    Ok(())
}

/// Rebuild a lost payload into its existing entry, never into a second entry with forgotten table decisions.
async fn payload_lost<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    mut entry: Entry,
) -> Result<(), String> {
    let n = entry.block_number;
    let block = target
        .stored_block(n)
        .await
        .map_err(|e| format!("block {n}: payload reconstruction failed: {e}"))?;
    if let Some(block) = &block {
        if block.number != n || block.hash.trim().to_ascii_lowercase() != entry.block_hash {
            return park_lost_payload(
                ingest,
                entry,
                format!(
                    "rebuilt block number/hash differs: {} {}",
                    block.number, block.hash
                ),
            );
        }
    }
    let Some(block) = block else {
        if entry.kind == EntryKind::Stream {
            ingest
                .journal
                .claim_gap(n, n, "journal-payload-lost", ingest_journal::wall_ms())
                .map_err(|e| format!("block {n}: gap claim failed: {e}"))?;
            error!(
                "TANK-ENGINE INGEST_JOURNAL_PAYLOAD_LOST block={}: waiting for the raw heal; the entry keeps every table's state",
                n
            );
            return Ok(());
        }
        return park_lost_payload(
            ingest,
            entry,
            "payload missing or corrupt and stored core rows unavailable".into(),
        );
    };
    let bytes = block.encode_to_vec();
    let batches = build_batches(&block, entry.now_ms);
    let actual = batches.tables(entry.kind);
    let expected: Vec<_> = entry
        .tables
        .iter()
        .map(|t| (t.table.as_str(), t.rows))
        .collect();
    let identical = entry.kind == EntryKind::Derive
        && bytes.len() as u64 == entry.payload_bytes
        && format!("{:016x}", ingest_journal::fnv1a64(&bytes)) == entry.payload_fnv1a64;
    if !identical && actual != expected {
        let why = format!(
            "rebuilt payload's table rows differ: expected {expected:?}, rebuilt {actual:?}"
        );
        return park_lost_payload(ingest, entry, why);
    }
    ingest
        .journal
        .restore_payload(&mut entry, &bytes)
        .map_err(|e| format!("block {n}: restore payload failed: {e}"))?;
    info!("TANK-ENGINE INGEST_JOURNAL_PAYLOAD_RESTORED block={}: table states, generations and send history kept", n);
    if entry.kind == EntryKind::Stream && stream_missing_traces(&block) {
        target
            .enqueue_trace_gap(&crate::healer::TraceGapRow {
                block_number: n,
                rpc_error_code: 0,
                reason: format!("stream-missing-traces: rebuilt block {n} is missing transaction traces"),
                attempted_at: ingest_journal::wall_ms(),
                resolved: 0,
                resolved_at: 0,
            })
            .await
            .map_err(|e| format!("block {n}: trace gap enqueue failed: {e}"))?;
        warn!(
            "TANK-ENGINE stream block={} rebuilt payload missing transaction traces; queued for the trace-gap healer, resuming the existing Stream owner",
            n
        );
    }
    complete_entry(target, ingest, &mut entry, &batches)
        .await
        .map(|_| ())
}

fn park_lost_payload(ingest: &Ingest, mut entry: Entry, why: String) -> Result<(), String> {
    let n = entry.block_number;
    entry.park_pending(&why);
    ingest
        .journal
        .save(&mut entry)
        .map_err(|e| format!("block {n}: save failed: {e}"))?;
    error!(
        "TANK-ENGINE INGEST_JOURNAL_PARKED block={} tables={}: {} — check the tables, then `tank-engine --ingest-journal resolve {} <table|all> done|retry` while tank-live is stopped",
        n, entry.parked().join(","), why, n
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Completion {
    /// Every table and the ledger row are written; the entry is removed.
    Finished,
    /// A table is parked for the operator; the entry stays.
    Parked,
}

/// `record_send` mutated `entry` and the following save failed before any insert. Undo only that last unsent id.
/// When the failed save's rename may already be durable, write the prior state back once. A failed rewrite
/// reloads the durable table instead of dropping its history. The owner is not finished.
pub(crate) fn revert_unsent_send(
    journal: &Journal,
    entry: &mut Entry,
    i: usize,
    query_id: &str,
    prior_first_send_ms: Option<u64>,
) -> Result<(), String> {
    let n = entry.block_number;
    let durable = match journal.entry(n) {
        Ok(Some(saved)) => saved.tables.iter().any(|t| {
            t.table == entry.tables[i].table && t.query_ids.iter().any(|id| id == query_id)
        }),
        Ok(None) => false,
        Err(_) => true,
    };
    let t = &mut entry.tables[i];
    debug_assert_eq!(t.query_ids.last().map(String::as_str), Some(query_id));
    t.sends -= 1;
    t.first_send_ms = prior_first_send_ms;
    t.query_ids.pop();
    t.clients.remove(query_id);
    if !durable || journal.save(entry).is_ok() {
        return Ok(());
    }
    if let Ok(Some(saved)) = journal.entry(n) {
        if let Some(saved_table) = saved
            .tables
            .into_iter()
            .find(|t| t.table == entry.tables[i].table)
        {
            entry.tables[i] = saved_table;
        }
    }
    Err(format!(
        "block {n} table {}: journal save failed before the send and unsent query_id {query_id} could not be removed; owner kept, nothing inserted",
        entry.tables[i].table,
    ))
}

/// Every transaction index needs a trace; duplicate or unrelated traces cannot stand in for a missing index.
pub(crate) fn trace_coverage_missing(
    mut tx_indices: impl Iterator<Item = u32>,
    mut has_trace: impl FnMut(u32) -> bool,
) -> bool {
    tx_indices.any(|index| !has_trace(index))
}

pub(crate) fn stream_missing_traces(block: &BlockNotification) -> bool {
    trace_coverage_missing(
        block.txs.iter().map(|tx| tx.tx_index),
        |index| block.traces.iter().any(|trace| trace.tx_index == index),
    )
}

/// Write every pending table in order. Each send's query_id and client identity are durable before `insert`, ahead
/// of any request byte: a streamed insert can commit a prefix before its body ends. Positive terminal evidence
/// fences every earlier send before reconciliation permits another request. A zero count alone never fences a late
/// send. Until the fence holds, the attempt stops without a send (Err, entry kept) for the journal's fence wait,
/// measured from the newest unresolved send; after it the table parks. The caller holds the block's lock. Err leaves
/// the entry for a later attempt.
pub(crate) async fn complete_entry<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    entry: &mut Entry,
    batches: &Batches,
) -> Result<Completion, String> {
    if entry.kind == EntryKind::Heal {
        return Err(format!(
            "block {}: a core-heal owner cannot write derived batches or the ledger",
            entry.block_number
        ));
    }
    let journal = &ingest.journal;
    let n = entry.block_number;
    let save = |entry: &mut Entry| {
        journal
            .save(entry)
            .map_err(|e| format!("block {n}: journal save failed: {e}"))
    };
    refresh_decided_sends(target, ingest, entry).await?;

    // Rows other writers already stored: core tables (raw heal, trace-gap healer) and the ledger row (the block is
    // finished), counted once, under the block's lock, before this entry first sends them.
    let ask: Vec<String> = entry
        .tables
        .iter()
        .filter(|t| t.status == TableStatus::Pending && t.sends == 0)
        .filter(|t| matches!(table_class(&t.table), TableClass::Core | TableClass::Ledger))
        .map(|t| t.table.clone())
        .collect();
    if !ask.is_empty() {
        let held = target
            .presence(entry, &ask)
            .await
            .map_err(|e| format!("block {n}: presence check failed: {e}"))?;
        let finished = held.iter().any(|(t, rows)| t == LEDGER_TABLE && *rows > 0);
        let mut changed = false;
        for t in entry
            .tables
            .iter_mut()
            .filter(|t| t.status == TableStatus::Pending && t.sends == 0)
        {
            let stored = held
                .iter()
                .any(|(table, rows)| *table == t.table && *rows > 0);
            if finished || stored {
                t.status = TableStatus::Done;
                t.note = Some(if finished {
                    "the ledger holds this block: every table was written".into()
                } else {
                    "the block's rows were already stored by another writer; not written again"
                        .into()
                });
                changed = true;
            }
        }
        if changed {
            save(entry)?;
            info!(
                "TANK-ENGINE ingest journal: block={} tables already stored: {:?}",
                n, held
            );
        }
    }

    let mut i = 0;
    while i < entry.tables.len() {
        if entry.tables[i].status != TableStatus::Pending {
            i += 1;
            continue;
        }
        let table = entry.tables[i].table.clone();
        if table == LEDGER_TABLE
            && entry
                .tables
                .iter()
                .any(|t| t.table != LEDGER_TABLE && t.status != TableStatus::Done)
        {
            i += 1;
            continue; // the ledger row only for a block whose every table is written
        }
        let now = ingest_journal::wall_ms();
        let token = ingest_journal::generation_token(
            &ingest_journal::dedup_token(&table, n, &entry.block_hash),
            entry.tables[i].generation,
        );
        if entry.tables[i].sends > 0 {
            // Two separate questions, never merged (C1 v3 review F2 vs F4):
            // F2, termination: until every recorded query_id is terminal or cannot still arrive, an earlier request
            //   may still commit, so nothing is counted, sent or advanced: wait (Err, entry kept), then park.
            // F4, commit: once every request is terminal, provable noncommit (typed connect failure,
            //   ExceptionBeforeStart, or a count that proves absence) retries without an operator; rows or a
            //   committed part are Done; anything else parks. A request still in flight can arrive after a retry's
            //   token is evicted, so the fence always precedes the count.
            let fenced = fence_sends(target, journal, entry, i).await?;
            if !fenced {
                if let Some(left) = journal.fence_wait_left(&entry.tables[i], now) {
                    return Err(fence_wait_error(n, &table, left));
                }
                park(
                    journal,
                    entry,
                    i,
                    format!("an earlier send has no terminal or cannot-arrive proof after the {}s fence wait; zero rows or process absence alone cannot fence a delayed request. A ClickHouse restart after its recorded client end proves it can no longer arrive; then resolve the table (DESIGN.md §1.6)", journal.fence_wait_ms() / 1000),
                )?;
                i += 1;
                continue;
            }
            let t = &entry.tables[i];
            let first = t.first_send_ms.unwrap_or(entry.created_at_ms);
            let mut found = None;
            // Every rejected request is terminal and never entered an insert pipeline. This is distinct from a
            // terminal processing exception, which may have committed a prefix and still needs reconciliation.
            let mut absence_proved = t.all_rejected();
            if !absence_proved {
                if let Some(sql) =
                    ingest_journal::count_rows_sql(&table, n, entry.block_timestamp, entry.now_ms)
                {
                    let rows = target
                        .count(&sql)
                        .await
                        .map_err(|e| format!("block {n} table {table}: count failed: {e}"))?;
                    if rows > 0 {
                        found = Some(format!(
                            "{rows} row(s) found after every send terminated; not sent again"
                        ));
                    } else {
                        absence_proved = ingest_journal::count_proves_absence(&table);
                    }
                }
                if found.is_none()
                    && !absence_proved
                    && target
                        .committed(&table, &token, &t.query_ids, first)
                        .await
                        .map_err(|e| {
                            format!("block {n} table {table}: commit evidence lookup failed: {e}")
                        })?
                {
                    found = Some("system.part_log shows a send committed and every send terminated; not sent again".to_string());
                }
            }
            if let Some(note) = found {
                let t = &mut entry.tables[i];
                t.status = TableStatus::Done;
                t.note = Some(note);
                save(entry)?;
                i += 1;
                continue;
            }
            if !absence_proved {
                let why = format!(
                    "sent {} time(s), the first {}s ago{}: every send terminated, but no positive commit evidence or proof of noncommit; missing logs cannot prove absence",
                    t.sends,
                    now.saturating_sub(first) / 1000,
                    if first < ingest.windows_since_ms { ", before the deduplication windows were set" } else { "" }
                );
                park(journal, entry, i, why)?;
                i += 1;
                continue;
            }
        }
        let query_id = ingest_journal::query_id(n, &table, now);
        // Durable before the first request byte. `insert.write` can flush a chunk that ClickHouse commits
        // before `Insert::end`, so a later write error is not proof the send never landed.
        let prior_first_send_ms = entry.tables[i].first_send_ms;
        entry.tables[i].record_send(&query_id, now)?;
        if let Err(err) = save(entry) {
            revert_unsent_send(journal, entry, i, &query_id, prior_first_send_ms)?;
            return Err(format!(
                "block {n}: journal save failed before {table} was sent: {err}; owner kept, nothing inserted"
            ));
        }
        match target
            .insert(batches, entry, &table, &token, &query_id)
            .await
        {
            Ok(()) => {
                entry.tables[i].record_client_end(&query_id, ingest_journal::wall_ms())?;
                entry.tables[i].record_terminal(&query_id, false);
                save(entry)?;
                if entry.tables[i].sends > 1 || table_class(&table) == TableClass::Core {
                    match confirm_resend(target, entry, &entry.tables[i], &token).await? {
                        Confirmed::Landed => {}
                        Confirmed::Unbacked => {
                            let t = &mut entry.tables[i];
                            t.next_generation()?;
                            t.note = Some(format!(
                                "generation {}: a send under generation {} was acknowledged and the table held none of the block's rows (a token recorded without its part)",
                                t.generation,
                                t.generation - 1
                            ));
                            let generation = t.generation;
                            save(entry)?;
                            error!(
                                "TANK-ENGINE INGEST_DEDUP_UNBACKED block={} table={}: the send under {} was acknowledged but the table holds none of the block's rows (a token recorded without its part, out/DEDUP-PROOF.md H1); sending them under generation {}",
                                n, table, token, generation
                            );
                            continue; // the same table, under the next generation
                        }
                        Confirmed::Unproven => {
                            park(
                                journal,
                                entry,
                                i,
                                format!(
                                    "a re-send under {token} was acknowledged and no system.part_log record shows a send of it committed: the rows landed once (their record lost), or the token is recorded without its part (out/DEDUP-PROOF.md H1)"
                                ),
                            )?;
                            i += 1;
                            continue;
                        }
                    }
                }
                let t = &mut entry.tables[i];
                t.status = TableStatus::Done;
                t.last_error = None;
                save(entry)?;
            }
            Err(e) => {
                entry.tables[i].record_client_end(&query_id, ingest_journal::wall_ms())?;
                if e.not_submitted {
                    entry.tables[i].record_terminal(&query_id, true);
                }
                entry.tables[i].last_error = Some(e.message.clone());
                if let Err(se) = journal.save(entry) {
                    warn!(
                        "TANK-ENGINE ingest journal: block={} table={} error record failed: {}",
                        n, table, se
                    );
                }
                return Err(format!(
                    "block {} insert {} failed: {} (journal entry kept; next attempt: fence every query_id, then rows/commit evidence = done; proven unsubmitted/rejected or global absence = same-token retry; otherwise parked)",
                    n, table, e
                ));
            }
        }
        i += 1;
    }

    let parked = entry.parked();
    if !parked.is_empty() {
        error!(
            "TANK-ENGINE INGEST_JOURNAL_PARKED block={} tables={}: the block is kept in the journal without its ledger row",
            n,
            parked.join(",")
        );
        return Ok(Completion::Parked);
    }
    if !entry.is_done() {
        return Err(format!("block {n}: tables left pending (internal error)"));
    }
    journal
        .finish(entry)
        .map_err(|e| format!("block {n}: finish failed: {e}"))?;
    info!(
        "TANK-ENGINE committed block={} kind={:?} txs={} logs={} traces={} transfers={} swaps={}",
        n,
        entry.kind,
        batches.txs.len(),
        batches.logs.len(),
        batches.traces.len(),
        batches.transfers.len(),
        batches.swaps.len()
    );
    Ok(Completion::Finished)
}

/// Parks table `i` of the entry for the operator: never sent again automatically.
fn park(journal: &Journal, entry: &mut Entry, i: usize, why: String) -> Result<(), String> {
    let n = entry.block_number;
    let table = entry.tables[i].table.clone();
    let t = &mut entry.tables[i];
    t.status = TableStatus::Parked;
    t.note = Some(why.clone());
    journal
        .save(entry)
        .map_err(|e| format!("block {n}: journal save failed: {e}"))?;
    error!(
        "TANK-ENGINE INGEST_JOURNAL_PARKED block={} table={}: {} — never sent again automatically; check the table, then `tank-engine --ingest-journal resolve {} {} done|retry` while tank-live is stopped",
        n, table, why, n, table
    );
    Ok(())
}

/// Done records from an older binary and explicit operator presence decisions are not a fence. Keep their owner
/// until every recorded request is terminal; parked entries may gain evidence but never automatically unpark.
async fn refresh_decided_sends<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    entry: &mut Entry,
) -> Result<(), String> {
    for i in 0..entry.tables.len() {
        if entry.tables[i].status == TableStatus::Pending || entry.tables[i].sends_fenced() {
            continue;
        }
        let fenced = fence_sends(target, &ingest.journal, entry, i).await?;
        if !fenced && entry.tables[i].status == TableStatus::Done {
            let now = ingest_journal::wall_ms();
            if let Some(left) = ingest.journal.fence_wait_left(&entry.tables[i], now) {
                return Err(fence_wait_error(
                    entry.block_number,
                    &entry.tables[i].table,
                    left,
                ));
            }
            park(&ingest.journal, entry, i,
                "rows were declared Done but an earlier query_id has no positive terminal evidence after the fence wait; owner retained".into())?;
        }
    }
    Ok(())
}

/// An unfenced table's attempt stops here: an earlier request may still terminate, and commit, later. Nothing is sent
/// and no generation changes while the fence wait lasts; the entry stays owned and the next attempt asks again.
pub(crate) fn fence_wait_error(block: u64, table: &str, left_ms: u64) -> String {
    format!(
        "INGEST_FENCE_WAIT block={block} table={table}: an earlier send has no positive terminal evidence yet; nothing is sent, the entry is kept and retried for up to {}s more, then the table is parked (TANK_FENCE_WAIT_SECS)",
        left_ms.div_ceil(1000)
    )
}

/// Resolve only under the writer flock, refreshing exact terminal evidence before either operator decision.
pub(crate) async fn resolve_journal(
    dir: &Path,
    lock_path: &Path,
    block: u64,
    table: &str,
    how: ingest_journal::Resolution,
) -> Result<String, String> {
    let journal = Journal::open(dir, lock_path).map_err(|e| e.to_string())?;
    let mut entry = journal
        .entry(block)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("block {block}: no durable owner"))?;
    let target = ClickHouseTarget::new(ch_client());
    for i in 0..entry.tables.len() {
        if if table == "all" {
            entry.tables[i].table == LEDGER_TABLE
        } else {
            entry.tables[i].table != table
        } {
            continue;
        }
        let fenced = fence_sends(&target, &journal, &mut entry, i).await?;
        if !fenced {
            return Err(format!("block {block} table {}: operator presence/absence cannot fence an earlier send; no terminal evidence for every query_id, and no ClickHouse restart since its client ended. The client ends are recorded now: restart ClickHouse, then resolve again (DESIGN.md §1.6)", entry.tables[i].table));
        }
    }
    ingest_journal::resolve_locked(&journal, block, table, how).map_err(|e| e.to_string())
}

/// Persist client-end observations before network I/O, and all fence evidence before callers may count or replay.
/// Log absence is used only after the sending client ended and a later server restart or host reboot excludes
/// arrival. Session receive settings cannot establish a listener arrival bound. A fence never proves noncommit.
pub(crate) async fn fence_sends<T: IngestTarget>(
    target: &T,
    journal: &Journal,
    entry: &mut Entry,
    i: usize,
) -> Result<bool, String> {
    if entry.tables[i].sends_fenced() {
        return Ok(true);
    }
    if entry.tables[i].observe_client_ends(ingest_journal::wall_ms())? {
        journal.save(entry).map_err(|e| e.to_string())?;
    }
    let t = &entry.tables[i];
    let unresolved: Vec<String> = t
        .query_ids
        .iter()
        .filter(|id| !t.terminal_query_ids.contains(id))
        .cloned()
        .collect();
    let mut changed = false;
    if !unresolved.is_empty() {
        let outcomes = target
            .terminal(&unresolved, t.first_send_ms.unwrap_or(entry.created_at_ms))
            .await
            .map_err(|e| {
                format!(
                    "block {} table {}: terminal evidence lookup failed: {e}",
                    entry.block_number, t.table
                )
            })?;
        for outcome in outcomes {
            if unresolved.contains(&outcome.query_id) {
                entry.tables[i].record_terminal(&outcome.query_id, outcome.rejected);
                changed = true;
            }
        }
        if !entry.tables[i].sends_fenced() {
            let proof = target.absent_sends(&entry.tables[i]).await.map_err(|e| {
                format!(
                    "block {} table {}: cannot-arrive evidence lookup failed: {e}",
                    entry.block_number, entry.tables[i].table
                )
            })?;
            let t = &mut entry.tables[i];
            for id in proof.query_ids {
                if unresolved.contains(&id) && !t.terminal_query_ids.contains(&id) {
                    // Lost logs are not rejection evidence. Unkeyed tables still require a commit record or an
                    // explicit operator decision, even after the request can no longer arrive.
                    t.record_terminal(&id, false);
                    changed = true;
                }
            }
        }
    }
    if changed {
        journal.save(entry).map_err(|e| e.to_string())?;
    }
    Ok(entry.tables[i].sends_fenced())
}

/// What an acknowledged send that could collide with an earlier use of its token shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirmed {
    /// The block's rows are in the table (counted), or a send of them committed (system.part_log).
    Landed,
    /// A count of every writer's rows finds none of the block's: the token is recorded without its part.
    Unbacked,
    /// No rows counted where that does not prove absence, and no record of a committed send: it can't be told apart
    /// from a lost record.
    Unproven,
}

/// Confirm an ACK only after every earlier request is terminal. A zero global count can then prove H1 without an
/// earlier request arriving under an obsolete generation later. Processing exceptions and absent logs never prove
/// that a request was rejected before committing a part.
async fn confirm_resend<T: IngestTarget>(
    target: &T,
    entry: &Entry,
    t: &TableState,
    token: &str,
) -> Result<Confirmed, String> {
    let n = entry.block_number;
    let table = &t.table;
    if !t.sends_fenced() {
        return Ok(Confirmed::Unproven);
    }
    if let Some(sql) = ingest_journal::count_rows_sql(table, n, entry.block_timestamp, entry.now_ms)
    {
        let rows = target
            .count(&sql)
            .await
            .map_err(|e| format!("block {n} table {table}: re-send count failed: {e}"))?;
        if rows > 0 {
            return Ok(Confirmed::Landed);
        }
        if ingest_journal::count_proves_absence(table) {
            return Ok(Confirmed::Unbacked);
        }
    }
    let since = t.first_send_ms.unwrap_or(entry.created_at_ms);
    let committed = target
        .committed(table, token, &t.query_ids, since)
        .await
        .map_err(|e| format!("block {n} table {table}: commit evidence lookup failed: {e}"))?;
    Ok(if committed {
        Confirmed::Landed
    } else {
        Confirmed::Unproven
    })
}

/// The healer's part of a gap claim: enqueue every chunk not yet acknowledged, then release the claim. A chunk
/// whose enqueue fails stays in the claim (durable) and is retried at the next call.
pub(crate) async fn process_gap_claims<T: IngestTarget>(
    target: &T,
    journal: &Journal,
) -> Result<(), String> {
    let claims = journal
        .claims()
        .map_err(|e| format!("gap claims unreadable: {e}"))?;
    for mut claim in claims {
        for (lo, hi) in claim.pending_chunks() {
            match target
                .enqueue_gap(lo, hi, claim.detected_at_ms, &claim.source)
                .await
            {
                Ok(()) => {
                    claim.enqueued.push([lo, hi]);
                    journal
                        .save_claim(&claim)
                        .map_err(|e| format!("gap claim [{lo}, {hi}] save failed: {e}"))?;
                    info!(
                        "TANK-ENGINE enqueued sequence gap [{}, {}] source={} (claim [{}, {}])",
                        lo, hi, claim.source, claim.lo, claim.hi
                    );
                }
                Err(e) => {
                    error!(
                        "TANK-ENGINE INGEST_GAP_ENQUEUE_RETRY gap=[{}, {}] (claim [{}, {}]): {} — the claim stays in the journal",
                        lo, hi, claim.lo, claim.hi, e
                    );
                    break;
                }
            }
        }
        if claim.pending_chunks().is_empty() {
            journal
                .release_claim(&claim)
                .map_err(|e| format!("gap claim release failed: {e}"))?;
        }
    }
    Ok(())
}

/// The gap deriver's part: derive a block the raw heal completed, from its stored core rows rebuilt as a block
/// (`backfiller::load_stored_block`). The caller holds the block's lock and has checked that the block is at or above
/// the cutover floor, owned by no journal entry and not in the ledger.
pub(crate) async fn derive_block<T: IngestTarget>(
    target: &T,
    ingest: &Ingest,
    block: &BlockNotification,
    now_ms: u64,
) -> Result<Completion, String> {
    let batches = build_batches(block, now_ms);
    let mut entry = Entry::new(
        EntryKind::Derive,
        block.number,
        &block.hash,
        block.timestamp,
        now_ms,
        &batches.tables(EntryKind::Derive),
    );
    ingest
        .journal
        .create(&mut entry, &block.encode_to_vec())
        .map_err(|e| format!("block {}: derive entry save failed: {}", block.number, e))?;
    complete_entry(target, ingest, &mut entry, &batches).await
}

/// A block below the cursor: ingested (or journaled) already, so it is not written again.
async fn report_overlap<T: IngestTarget>(target: &T, block: &BlockNotification, expected: u64) {
    match target.stored_hash(block.number).await {
        Ok(Some(h)) if h == block.hash.to_lowercase() => warn!(
            "TANK-ENGINE INGEST_OVERLAP block={} below expected={} (same hash): already ingested, not written again",
            block.number, expected
        ),
        Ok(Some(h)) => error!(
            "TANK-ENGINE INGEST_OVERLAP_HASH_MISMATCH block={} delivered={} stored={} below expected={}: not written; the streamer does not handle reorgs, reconcile by hand",
            block.number, block.hash, h, expected
        ),
        Ok(None) => warn!(
            "TANK-ENGINE INGEST_OVERLAP block={} hash={} below expected={}: no block row stored yet (journaled or claimed as a gap); not written again",
            block.number, block.hash, expected
        ),
        Err(e) => warn!(
            "TANK-ENGINE INGEST_OVERLAP block={} hash={} below expected={}: stored hash unavailable ({}); not written again",
            block.number, block.hash, expected, e
        ),
    }
}

/// C1 v2 schema check (fail closed): every table the engine writes idempotently keeps a deduplication window of at
/// least MIN_DEDUP_WINDOW, and the ledger table exists.
pub(crate) async fn check_schema(client: &Client) -> Result<(), String> {
    #[derive(Debug, clickhouse::Row, serde::Deserialize)]
    struct TableDef {
        name: String,
        engine_full: String,
    }
    let names = ingest_journal::DEDUP_TABLES
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(",");
    let defs: Vec<TableDef> = client
        .query(&format!(
            "SELECT name, engine_full FROM system.tables WHERE database = '{CLICKHOUSE_DB}' AND name IN ({names})"
        ))
        .fetch_all()
        .await
        .map_err(|e| format!("system.tables query failed: {e}"))?;
    schema_problems(
        &defs
            .iter()
            .map(|d| (d.name.as_str(), d.engine_full.as_str()))
            .collect::<Vec<_>>(),
    )
}

/// The problems of the schema described by (table, engine_full) pairs; Ok when there are none.
fn schema_problems(defs: &[(&str, &str)]) -> Result<(), String> {
    let mut problems = Vec::new();
    if !defs.iter().any(|(name, _)| *name == LEDGER_TABLE) {
        problems.push(format!("worldchain.{LEDGER_TABLE} missing"));
    }
    for (name, engine_full) in defs {
        let window = dedup_window(engine_full);
        if window < ingest_journal::MIN_DEDUP_WINDOW {
            problems.push(format!(
                "{name}: non_replicated_deduplication_window {window} < {}",
                ingest_journal::MIN_DEDUP_WINDOW
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// The table's non_replicated_deduplication_window from its engine_full (0 when not set).
fn dedup_window(engine_full: &str) -> u64 {
    const KEY: &str = "non_replicated_deduplication_window = ";
    engine_full
        .find(KEY)
        .map(|i| &engine_full[i + KEY.len()..])
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// Every row one block writes, built from the block and its ingested_at alone: a resumed block rebuilds the same
/// rows from its saved payload.
pub(crate) struct Batches {
    blocks: Vec<BlockRow>,
    txs: Vec<TransactionRow>,
    logs: Vec<LogRow>,
    traces: Vec<TraceRow>,
    transfers: Vec<TokenTransferRow>,
    swaps: Vec<DexSwapRow>,
    universal_events: Vec<UniversalDefiEventRow>,
    dex_swap_details: Vec<ApiDexSwapDetailRow>,
    gas_sponsorships: Vec<ApiGasSponsorshipRow>,
    humanity_feeds: Vec<ApiHumanityFeedRow>,
    nft_transfers: Vec<ApiNftTransferRow>,
    oracle_prices: Vec<ApiOraclePriceRow>,
    erc1155_transfers: Vec<WorldErc1155TransferRow>,
    internal_transfers: Vec<ApiInternalTransferRow>,
    v3_prices: Vec<ApiUniswapV3PriceRow>,
    whale_alerts: Vec<ApiWhaleAlertRow>,
    contract_interactions: Vec<WorldContractInteractionRow>,
    gas_by_function: Vec<ApiGasByFunctionRow>,
    lending_liquidations: Vec<ApiLendingLiquidationRow>,
    mev_stats: Vec<WorldMevHourlyStatsRow>,
    token_balances: Vec<ApiTokenBalanceRow>,
    latest_oracle_prices: Vec<LatestOraclePriceRow>,
    uniswap_mints: Vec<ApiUniswapMintRow>,
    morpho_activity: Vec<ApiMorphoActivityRow>,
}

impl Batches {
    /// Tables this block has rows for, in write order: for a streamed block the transactions, logs, traces, then
    /// the block row (last of the core tables, as in the fixed raw heal), then the derived tables in the streamer's
    /// historical order; for a derive entry only the derived tables. The ledger row is always last.
    pub(crate) fn tables(&self, kind: EntryKind) -> Vec<(&'static str, u64)> {
        self.manifest()
            .into_iter()
            .skip(if kind == EntryKind::Stream { 0 } else { 4 })
            .filter(|(_, n)| *n > 0)
            .chain(std::iter::once((LEDGER_TABLE, 1)))
            .collect()
    }

    /// Complete expected rows for the block, independent of which writer supplied them. Zero counts matter to
    /// verification too: any row in a table expected to be empty is a mismatch.
    pub(crate) fn manifest(&self) -> [(&'static str, u64); 24] {
        [
            ("world_transactions", self.txs.len() as u64),
            ("world_logs", self.logs.len() as u64),
            ("world_traces", self.traces.len() as u64),
            ("world_blocks", self.blocks.len() as u64),
            ("world_token_transfers", self.transfers.len() as u64),
            ("api_live_dex_swaps", self.swaps.len() as u64),
            (
                "api_universal_defi_events",
                self.universal_events.len() as u64,
            ),
            ("api_dex_swaps", self.dex_swap_details.len() as u64),
            ("api_gas_sponsorship", self.gas_sponsorships.len() as u64),
            ("api_humanity_feed", self.humanity_feeds.len() as u64),
            ("api_nft_transfers", self.nft_transfers.len() as u64),
            ("api_oracle_prices", self.oracle_prices.len() as u64),
            (
                "world_erc1155_transfers",
                self.erc1155_transfers.len() as u64,
            ),
            (
                "api_internal_transfers",
                self.internal_transfers.len() as u64,
            ),
            ("api_uniswap_v3_prices", self.v3_prices.len() as u64),
            ("api_whale_alerts", self.whale_alerts.len() as u64),
            (
                "world_contract_interactions",
                self.contract_interactions.len() as u64,
            ),
            ("api_gas_by_function", self.gas_by_function.len() as u64),
            (
                "api_lending_liquidations",
                self.lending_liquidations.len() as u64,
            ),
            ("world_mev_hourly_stats", self.mev_stats.len() as u64),
            ("api_token_balances", self.token_balances.len() as u64),
            (
                "latest_oracle_prices",
                self.latest_oracle_prices.len() as u64,
            ),
            ("api_uniswap_mints", self.uniswap_mints.len() as u64),
            ("api_morpho_activity", self.morpho_activity.len() as u64),
        ]
    }
}

/// The per-block row building of the streamer, unchanged except that ingested_at comes from the caller.
pub(crate) fn build_batches(block: &BlockNotification, now_ms: u64) -> Batches {
    let now_s = (now_ms / 1000) as u32;

    let block_num = block.number;
    let block_ts = block.timestamp as u32;
    let hour_bucket = (block_ts / 3600) * 3600;

    let mut blocks_batch = Vec::new();
    let mut txs_batch = Vec::new();
    let mut logs_batch = Vec::new();
    let mut traces_batch = Vec::new();

    let mut transfers_batch = Vec::new();
    let mut swaps_batch = Vec::new();
    let mut universal_events_batch = Vec::new();
    let mut dex_swap_details_batch = Vec::new();
    let mut gas_sponsorships_batch = Vec::new();
    let mut humanity_feeds_batch = Vec::new();
    let mut nft_transfers_batch = Vec::new();
    let mut oracle_prices_batch = Vec::new();
    let mut erc1155_transfers_batch = Vec::new();
    let mut internal_transfers_batch = Vec::new();
    let mut v3_prices_batch = Vec::new();
    let mut whale_alerts_batch = Vec::new();
    let mut contract_interactions_batch = Vec::new();
    let mut gas_by_function_batch = Vec::new();
    let mut lending_liquidations_batch = Vec::new();
    let mut mev_stats_batch = Vec::new();
    let mut token_balances_batch = Vec::new();
    let mut latest_oracle_prices_batch = Vec::new();
    let mut uniswap_mints_batch = Vec::new();
    let mut morpho_activity_batch = Vec::new();

    blocks_batch.push(BlockRow {
        chain_id: 480,
        block_number: block_num,
        block_hash: str_to_fixed_66(&block.hash),
        parent_hash: str_to_fixed_66(&block.parent_hash),
        timestamp: block.timestamp * 1000,
        miner: str_to_fixed_42(&block.miner),
        gas_limit: block.gas_limit,
        gas_used: block.gas_used,
        base_fee_per_gas: block.base_fee,
        transactions_count: block.tx_count,
        size_bytes: block.size_bytes,
        ingested_at: now_ms,
        is_canonical: 1,
        healed_by: "tank-engine-rust".to_string(),
    });

    for tx in &block.txs {
        txs_batch.push(TransactionRow {
            chain_id: 480,
            block_number: block_num,
            block_hash: str_to_fixed_66(&block.hash),
            tx_index: tx.tx_index,
            tx_hash: str_to_fixed_66(&tx.hash),
            from_address: str_to_fixed_42(&tx.from),
            to_address: str_to_fixed_42(&tx.to),
            value_wei: parse_u256_dec_bytes(&tx.value),
            mint_wei: parse_u256_dec_bytes(&tx.mint_wei),
            gas_price_wei: tx.gas_price,
            max_fee_per_gas_wei: tx.max_fee_per_gas,
            max_priority_fee_wei: tx.max_priority_fee,
            gas_limit: tx.gas_limit,
            gas_used: tx.gas_used,
            nonce: tx.nonce,
            tx_type: tx.tx_type as u8,
            is_system_tx: if tx.is_system_tx { 1 } else { 0 },
            input_data: tx.input.clone(),
            status: tx.status as u8,
            effective_gas_price_wei: tx.gas_price,
            ingested_at: now_ms,
            block_timestamp: block_ts,
            // the ExEx sends l1_fee as a decimal U256 string (l1_fee.to_string()); a hex parse stored every
            // live fee wrong from 2026-09 to 2026-10-13
            l1_fee_wei: parse_u256_dec_bytes(&tx.l1_fee),
            l1_gas_price_wei: tx.l1_gas_price,
            is_canonical: 1,
            healed_by: "tank-engine-rust".to_string(),
        });

        if !tx.to.is_empty() && tx.input.len() >= 10 {
            let fn_sel = str_to_fixed_10(&tx.input);
            let to_addr = str_to_fixed_42(&tx.to);

            contract_interactions_batch.push(WorldContractInteractionRow {
                chain_id: 480,
                from_address: str_to_fixed_42(&tx.from),
                to_address: to_addr.clone(),
                function_selector: fn_sel.clone(),
                call_count: 1,
                total_value_wei: parse_u256_dec_bytes(&tx.value),
                error_count: if tx.status == 0 { 1 } else { 0 },
                last_seen_block: block_num,
                updated_at: block_ts,
            });

            gas_by_function_batch.push(ApiGasByFunctionRow {
                address: to_addr,
                function_selector: fn_sel,
                call_count: 1,
                total_gas_used: tx.gas_used,
                avg_gas_used: tx.gas_used as f64,
                min_gas_used: tx.gas_used,
                max_gas_used: tx.gas_used,
                success_count: if tx.status == 1 { 1 } else { 0 },
                revert_count: if tx.status == 0 { 1 } else { 0 },
                updated_at: block_ts,
            });
        }
    }

    for log in &block.logs {
        logs_batch.push(LogRow {
            chain_id: 480,
            block_number: block_num,
            block_hash: str_to_fixed_66(&block.hash),
            tx_hash: str_to_fixed_66(&log.tx_hash),
            tx_index: log.tx_index,
            log_index: log.log_index,
            address: str_to_fixed_42(&log.address),
            data: log.data.clone(),
            topics: log.topics.iter().map(|t| str_to_fixed_66(t)).collect(),
            ingested_at: now_ms,
            block_timestamp: block_ts,
            is_canonical: 1,
            healed_by: "tank-engine-rust".to_string(),
        });

        let mut extracted = match_and_extract_views(log, block_num, block_ts, now_ms, now_s);

        if let Some(t) = extracted.transfer {
            transfers_batch.push(t);
        }
        if let Some(s) = extracted.dex_swap {
            swaps_batch.push(s);
        }
        if let Some(u) = extracted.universal_event {
            universal_events_batch.push(u);
        }
        if let Some(d) = extracted.dex_swap_detail {
            dex_swap_details_batch.push(d);
        }
        if let Some(g) = extracted.gas_sponsorship {
            gas_sponsorships_batch.push(g);
        }
        if let Some(h) = extracted.humanity_feed {
            humanity_feeds_batch.push(h);
        }
        if let Some(n) = extracted.nft_transfer {
            nft_transfers_batch.push(n);
        }
        if let Some(o) = extracted.oracle_price {
            oracle_prices_batch.push(o);
        }
        if let Some(e) = extracted.erc1155_transfer {
            erc1155_transfers_batch.push(e);
        }
        if let Some(vp) = extracted.uniswap_v3_price {
            v3_prices_batch.push(vp);
        }
        if let Some(w) = extracted.whale_alert {
            whale_alerts_batch.push(w);
        }
        if let Some(l) = extracted.lending_liquidation {
            lending_liquidations_batch.push(l);
        }
        if let Some(lo) = extracted.latest_oracle_price {
            latest_oracle_prices_batch.push(lo);
        }
        if let Some(um) = extracted.uniswap_mint {
            uniswap_mints_batch.push(um);
        }
        if let Some(ma) = extracted.morpho_activity {
            morpho_activity_batch.push(ma);
        }

        token_balances_batch.append(&mut extracted.token_balances);
    }

    for trace in &block.traces {
        traces_batch.push(TraceRow {
            chain_id: 480,
            block_number: block_num,
            block_hash: str_to_fixed_66(&block.hash),
            tx_index: trace.tx_index,
            tx_hash: str_to_fixed_66(&trace.tx_hash),
            from_address: str_to_fixed_42(&trace.from_address),
            to_address: str_to_fixed_42(&trace.to_address),
            value_wei: parse_u256_dec_bytes(&trace.value_wei),
            input_data: trace.input_data.clone(),
            output_data: trace.output_data.clone(),
            gas_limit: trace.gas,
            gas_used: trace.gas_used,
            trace_type: trace.trace_type.clone(),
            call_type: trace.call_type.clone(),
            error: trace.revert_reason.clone(),
            depth: 0,
            ingested_at: now_ms,
            block_timestamp: block_ts,
            is_canonical: 1,
            trace_index: trace.trace_index,
            healed_by: "tank-engine-rust".to_string(),
        });

        if (trace.trace_type.eq_ignore_ascii_case("call")
            || trace.trace_type.eq_ignore_ascii_case("create"))
            && !trace.value_wei.is_empty()
            && trace.value_wei != "0"
        {
            internal_transfers_batch.push(ApiInternalTransferRow {
                ingested_at: now_s,
                block_number: block_num,
                tx_hash: str_to_fixed_66(&trace.tx_hash),
                from_address: str_to_fixed_42(&trace.from_address),
                to_address: str_to_fixed_42(&trace.to_address),
                value_wei: parse_u256_dec_bytes(&trace.value_wei),
                depth: 0,
            });
        }
    }

    if swaps_batch.len() >= 2 {
        mev_stats_batch.push(WorldMevHourlyStatsRow {
            timestamp_hour: hour_bucket,
            mev_type: "ARBITRAGE".to_string(),
            total_events: 1,
            total_extracted_wei: [0u8; 32],
            max_extracted_wei: [0u8; 32],
        });
    }

    Batches {
        blocks: blocks_batch,
        txs: txs_batch,
        logs: logs_batch,
        traces: traces_batch,
        transfers: transfers_batch,
        swaps: swaps_batch,
        universal_events: universal_events_batch,
        dex_swap_details: dex_swap_details_batch,
        gas_sponsorships: gas_sponsorships_batch,
        humanity_feeds: humanity_feeds_batch,
        nft_transfers: nft_transfers_batch,
        oracle_prices: oracle_prices_batch,
        erc1155_transfers: erc1155_transfers_batch,
        internal_transfers: internal_transfers_batch,
        v3_prices: v3_prices_batch,
        whale_alerts: whale_alerts_batch,
        contract_interactions: contract_interactions_batch,
        gas_by_function: gas_by_function_batch,
        lending_liquidations: lending_liquidations_batch,
        mev_stats: mev_stats_batch,
        token_balances: token_balances_batch,
        latest_oracle_prices: latest_oracle_prices_batch,
        uniswap_mints: uniswap_mints_batch,
        morpho_activity: morpho_activity_batch,
    }
}

/// The ledger row of a finished block: the tables written and their row counts (the expected-row evidence the
/// deploy verify checks against the tables).
#[derive(Debug, Clone, clickhouse::Row, serde::Serialize)]
pub(crate) struct LedgerRow {
    pub block_number: u64,
    pub block_hash: String,
    pub source: String,
    pub ingested_at: u64,
    pub finished_at: u64,
    pub tables: Vec<String>,
    pub table_rows: Vec<u64>,
}

impl LedgerRow {
    /// All expected tables and counts, including core supplied by the raw heal and tables with zero expected rows.
    fn of(entry: &Entry, batches: &Batches) -> LedgerRow {
        let written = batches.manifest();
        LedgerRow {
            block_number: entry.block_number,
            block_hash: entry.block_hash.clone(),
            source: match entry.kind {
                EntryKind::Stream => "stream".into(),
                EntryKind::Derive => "derive".into(),
                EntryKind::Heal => unreachable!("core-heal owners cannot write a ledger row"),
            },
            ingested_at: entry.now_ms,
            finished_at: ingest_journal::wall_ms(),
            tables: written
                .iter()
                .map(|(table, _)| (*table).to_string())
                .collect(),
            table_rows: written.iter().map(|(_, rows)| *rows).collect(),
        }
    }
}

/// Submission certainty is structured, never parsed from an arbitrary HTTP exception string. `not_submitted` is
/// true only for a typed connector connect failure (`hyper_util::client::legacy::Error::is_connect`): the
/// connection never opened. A later network error, including `insert.write`, is uncertain. ClickHouse can commit
/// one block of a streamed insert before the terminating chunk.
#[derive(Debug)]
pub(crate) struct InsertFailure {
    pub message: String,
    pub not_submitted: bool,
}

impl std::fmt::Display for InsertFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InsertFailure {}

impl From<clickhouse::error::Error> for InsertFailure {
    fn from(error: clickhouse::error::Error) -> Self {
        let not_submitted = match &error {
            clickhouse::error::Error::Network(source) => source
                .downcast_ref::<hyper_util::client::legacy::Error>()
                .is_some_and(|error| error.is_connect()),
            _ => false,
        };
        Self {
            message: error.to_string(),
            not_submitted,
        }
    }
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
pub(crate) struct TerminalSend {
    pub query_id: String,
    pub rejected: bool,
}

#[derive(Debug, Default)]
pub(crate) struct AbsentSends {
    query_ids: Vec<String>,
}

/// A conservative server-start lower bound on the local raw clock, valid only on this host boot.
#[derive(Debug, Clone, Copy)]
struct ServerFence {
    boot_id: &'static str,
    start_lower_raw_ms: u64,
}

impl ServerFence {
    fn from_uptime(
        boot_id: &'static str,
        before_raw_ms: u64,
        uptime_s: u64,
    ) -> Result<Self, String> {
        // uptime() is truncated to seconds and evaluated after the local sample: subtract an entire extra second.
        // Checked subtraction also verifies that the server uptime is possible on this local host boot.
        let elapsed_ms = uptime_s
            .checked_add(1)
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or_else(|| "server uptime exceeds CLOCK_MONOTONIC_RAW range".to_string())?;
        let start_lower_raw_ms = before_raw_ms.checked_sub(elapsed_ms).ok_or_else(|| {
            "server uptime exceeds local CLOCK_MONOTONIC_RAW boot epoch".to_string()
        })?;
        Ok(Self {
            boot_id,
            start_lower_raw_ms,
        })
    }
}

fn absence_candidates(t: &TableState, server: ServerFence) -> AbsentSends {
    let mut proof = AbsentSends::default();
    for id in t
        .query_ids
        .iter()
        .filter(|id| !t.terminal_query_ids.contains(id))
    {
        let Some(client) = t.clients.get(id) else {
            continue;
        };
        if client.ended_ms.is_none() {
            continue;
        }
        // A different host boot destroyed every old connection; its raw epoch is unrelated to the current one.
        // Otherwise startup must be strictly later than the rounded-up client-end observation.
        if client.boot_id != server.boot_id
            || client
                .ended_raw_ms
                .is_some_and(|ended| server.start_lower_raw_ms > ended)
        {
            proof.query_ids.push(id.clone());
        }
    }
    proof
}

/// Where a block's rows go (ClickHouse in production; a fake in the unit tests).
pub(crate) trait IngestTarget {
    /// One table's rows of the block, synchronous, under its deduplication token. The caller has already made this
    /// send's query_id durable. `not_submitted` is true only for a typed connect failure; a write or end error is
    /// uncertain because an earlier chunk can commit before the body ends.
    async fn insert(
        &self,
        batches: &Batches,
        entry: &Entry,
        table: &str,
        token: &str,
        query_id: &str,
    ) -> Result<(), InsertFailure>;
    /// Rows the block has in each of `tables` (core tables and the ledger: any writer's).
    async fn presence(
        &self,
        entry: &Entry,
        tables: &[String],
    ) -> Result<Vec<(String, u64)>, String>;
    /// `SELECT ... AS n` (one row).
    async fn count(&self, sql: &str) -> Result<u64, String>;
    /// One chunk of a gap into world_sequence_gaps (acknowledged when Ok).
    async fn enqueue_gap(
        &self,
        lo: u64,
        hi: u64,
        detected_at_ms: u64,
        source: &str,
    ) -> Result<(), String>;
    /// One unresolved height into the existing world_trace_gaps queue (acknowledged when Ok).
    async fn enqueue_trace_gap(&self, row: &crate::healer::TraceGapRow) -> Result<(), String>;
    /// The stored hash of a block row (lowercase), if any.
    async fn stored_hash(&self, block: u64) -> Result<Option<String>, String>;
    /// Source block rebuilt from stored core rows, if they are ready. Recovery retains the entry's ingested_at.
    async fn stored_block(&self, block: u64) -> Result<Option<BlockNotification>, String>;
    /// Positive evidence that a send of a table's rows committed into the table as it is now: a `system.part_log`
    /// NewPart record without error, of the table's current incarnation (its UUID), whose query_id is one of
    /// `query_ids`, or of a query that carried `token` (`system.query_log`), from the day before `since_ms` on. False
    /// proves nothing: both logs are best-effort.
    async fn committed(
        &self,
        table: &str,
        token: &str,
        query_ids: &[String],
        since_ms: u64,
    ) -> Result<bool, String>;
    /// Positive query-log termination for each exact request ID. ExceptionBeforeStart alone proves no pipeline ran;
    /// QueryFinish and ExceptionWhileProcessing only fence the request and may have committed rows.
    async fn terminal(
        &self,
        query_ids: &[String],
        since_ms: u64,
    ) -> Result<Vec<TerminalSend>, String>;
    /// IDs with no process or query-log row of any type after flush, after a later server restart or host reboot
    /// proves an ended client cannot still reach the server. Fail closed on unavailable clocks or privileges.
    async fn absent_sends(&self, t: &TableState) -> Result<AbsentSends, String>;
}

/// The query behind `ClickHouseTarget::committed` (one row, `n`). The deduplicated re-send of a landed block has its
/// own NewPart record with error 389 (INSERT_WAS_DEDUPLICATED), never 0; a query_log record's written_rows counts
/// the rows of a deduplicated insert too, so only part_log decides. The logs outlive a dropped table: a record of an
/// earlier table of the same name (same token, other UUID) is not evidence.
pub(crate) fn committed_sql(
    table: &str,
    token: &str,
    query_ids: &[String],
    since_ms: u64,
) -> String {
    let lit = |s: &str| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"));
    let day = format!("toDate(toDateTime({})) - 1", since_ms / 1000);
    let by_id = if query_ids.is_empty() {
        "0".to_string()
    } else {
        format!(
            "query_id IN ({})",
            query_ids
                .iter()
                .map(|q| lit(q))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "SELECT toUInt64(count()) AS n FROM system.part_log \
         WHERE event_date >= {day} AND event_type = 'NewPart' AND error = 0 AND database = {db} AND table = {table} \
         AND table_uuid = (SELECT uuid FROM system.tables WHERE database = {db} AND name = {table}) \
         AND ({by_id} OR query_id IN (SELECT query_id FROM system.query_log WHERE event_date >= {day} \
         AND type = 'QueryFinish' AND query_kind = 'Insert' AND Settings['insert_deduplication_token'] = {token}))",
        db = lit(CLICKHOUSE_DB),
        table = lit(table),
        token = lit(token),
    )
}

pub(crate) struct ClickHouseTarget {
    client: Client,
}

impl ClickHouseTarget {
    pub(crate) fn new(client: Client) -> ClickHouseTarget {
        ClickHouseTarget { client }
    }
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Count {
    n: u64,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct TableCount {
    t: String,
    n: u64,
}

impl IngestTarget for ClickHouseTarget {
    async fn insert(
        &self,
        batches: &Batches,
        entry: &Entry,
        table: &str,
        token: &str,
        query_id: &str,
    ) -> Result<(), InsertFailure> {
        insert_table(&self.client, batches, entry, table, token, query_id).await
    }

    async fn presence(
        &self,
        entry: &Entry,
        tables: &[String],
    ) -> Result<Vec<(String, u64)>, String> {
        let parts: Vec<String> = tables
            .iter()
            .filter_map(|t| {
                ingest_journal::count_rows_sql(
                    t,
                    entry.block_number,
                    entry.block_timestamp,
                    entry.now_ms,
                )
                .map(|sql| format!("SELECT '{t}' AS t, n FROM ({sql})"))
            })
            .collect();
        if parts.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<TableCount> = self
            .client
            .query(&parts.join(" UNION ALL "))
            .fetch_all()
            .await
            .map_err(|e| e.to_string())?;
        if rows.len() != parts.len() {
            return Err(format!(
                "presence query returned {} of {} rows",
                rows.len(),
                parts.len()
            ));
        }
        Ok(rows.into_iter().map(|r| (r.t, r.n)).collect())
    }

    async fn count(&self, sql: &str) -> Result<u64, String> {
        Ok(self
            .client
            .query(sql)
            .fetch_one::<Count>()
            .await
            .map_err(|e| e.to_string())?
            .n)
    }

    async fn enqueue_gap(
        &self,
        lo: u64,
        hi: u64,
        detected_at_ms: u64,
        source: &str,
    ) -> Result<(), String> {
        crate::raw_healer::enqueue_gap_rows(&self.client, &[(lo, hi)], detected_at_ms, source)
            .await
            .map_err(|e| e.to_string())
    }

    async fn enqueue_trace_gap(&self, row: &crate::healer::TraceGapRow) -> Result<(), String> {
        crate::raw_healer::insert_queue_row(&self.client, "world_trace_gaps", row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn stored_hash(&self, block: u64) -> Result<Option<String>, String> {
        #[derive(Debug, clickhouse::Row, serde::Deserialize)]
        struct StoredHash {
            h: String,
        }
        let sql = format!(
            "SELECT lower(toString(block_hash)) AS h FROM worldchain.world_blocks WHERE chain_id = 480 AND block_number = {block} LIMIT 1"
        );
        Ok(self
            .client
            .query(&sql)
            .fetch_optional::<StoredHash>()
            .await
            .map_err(|e| e.to_string())?
            .map(|s| s.h))
    }

    async fn committed(
        &self,
        table: &str,
        token: &str,
        query_ids: &[String],
        since_ms: u64,
    ) -> Result<bool, String> {
        self.client
            .query("SYSTEM FLUSH LOGS part_log, query_log")
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        Ok(self
            .count(&committed_sql(table, token, query_ids, since_ms))
            .await?
            > 0)
    }

    async fn terminal(
        &self,
        query_ids: &[String],
        since_ms: u64,
    ) -> Result<Vec<TerminalSend>, String> {
        if query_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.client
            .query("SYSTEM FLUSH LOGS query_log")
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        let ids = query_ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\\', "\\\\").replace('\'', "\\'")))
            .collect::<Vec<_>>()
            .join(",");
        self.client.query(&format!(
            "SELECT query_id, countIf(type = 'ExceptionBeforeStart') > 0 AND countIf(type IN ('QueryFinish', 'ExceptionWhileProcessing')) = 0 AS rejected \
             FROM system.query_log WHERE event_date >= toDate(toDateTime({})) - 1 AND is_initial_query = 1 \
             AND query_id IN ({ids}) AND type IN ('QueryFinish', 'ExceptionBeforeStart', 'ExceptionWhileProcessing') GROUP BY query_id",
            since_ms / 1000,
        )).fetch_all().await.map_err(|e| e.to_string())
    }

    async fn absent_sends(&self, t: &TableState) -> Result<AbsentSends, String> {
        if !t.query_ids.iter().any(|id| {
            !t.terminal_query_ids.contains(id)
                && t.clients.get(id).is_some_and(|c| c.ended_ms.is_some())
        }) {
            return Ok(AbsentSends::default());
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct Snapshot {
            uptime_s: u64,
        }
        let boot_id = ingest_journal::current_boot_id()?;
        // Sample before requesting uptime, not after: network/scheduler delay can only make the bound earlier.
        let before_raw_ms = ingest_journal::monotonic_raw_ms()?;
        let snapshot = self
            .client
            .query("SELECT toUInt64(uptime()) AS uptime_s")
            .fetch_one::<Snapshot>()
            .await
            .map_err(|e| e.to_string())?;
        let server = ServerFence::from_uptime(boot_id, before_raw_ms, snapshot.uptime_s)?;
        let mut proof = absence_candidates(t, server);
        if proof.query_ids.is_empty() {
            return Ok(proof);
        }
        // No privileges, missing logs or query failures are not an absence result.
        self.client
            .query("SYSTEM FLUSH LOGS")
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        let ids = proof
            .query_ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\\', "\\\\").replace('\'', "\\'")))
            .collect::<Vec<_>>()
            .join(",");
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct Seen {
            id: String,
        }
        // Any log type blocks this fence, including QueryStart. No date/type/is_initial_query filter can hide a row.
        // Include initial_query_id so a delegated request also remains visible.
        let seen = self
            .client
            .query(&format!(
                "SELECT arrayJoin([query_id, initial_query_id]) AS id FROM system.processes \
             WHERE query_id IN ({ids}) OR initial_query_id IN ({ids}) \
             UNION ALL \
             SELECT arrayJoin([query_id, initial_query_id]) AS id FROM system.query_log \
             WHERE query_id IN ({ids}) OR initial_query_id IN ({ids})"
            ))
            .fetch_all::<Seen>()
            .await
            .map_err(|e| e.to_string())?;
        proof
            .query_ids
            .retain(|id| !seen.iter().any(|row| &row.id == id));
        Ok(proof)
    }

    async fn stored_block(&self, block: u64) -> Result<Option<BlockNotification>, String> {
        Ok(crate::backfiller::load_stored_block(&self.client, block)
            .await?
            .map(|(block, _)| block))
    }
}

async fn insert_table(
    client: &Client,
    b: &Batches,
    entry: &Entry,
    table: &str,
    token: &str,
    query_id: &str,
) -> Result<(), InsertFailure> {
    let ledger = if table == LEDGER_TABLE {
        vec![LedgerRow::of(entry, b)]
    } else {
        Vec::new()
    };
    match table {
        "world_transactions" => insert_rows(client, table, &b.txs, token, query_id).await,
        "world_logs" => insert_rows(client, table, &b.logs, token, query_id).await,
        "world_traces" => insert_rows(client, table, &b.traces, token, query_id).await,
        "world_blocks" => insert_rows(client, table, &b.blocks, token, query_id).await,
        "world_token_transfers" => insert_rows(client, table, &b.transfers, token, query_id).await,
        "api_live_dex_swaps" => insert_rows(client, table, &b.swaps, token, query_id).await,
        "api_universal_defi_events" => {
            insert_rows(client, table, &b.universal_events, token, query_id).await
        }
        "api_dex_swaps" => insert_rows(client, table, &b.dex_swap_details, token, query_id).await,
        "api_gas_sponsorship" => {
            insert_rows(client, table, &b.gas_sponsorships, token, query_id).await
        }
        "api_humanity_feed" => insert_rows(client, table, &b.humanity_feeds, token, query_id).await,
        "api_nft_transfers" => insert_rows(client, table, &b.nft_transfers, token, query_id).await,
        "api_oracle_prices" => insert_rows(client, table, &b.oracle_prices, token, query_id).await,
        "world_erc1155_transfers" => {
            insert_rows(client, table, &b.erc1155_transfers, token, query_id).await
        }
        "api_internal_transfers" => {
            insert_rows(client, table, &b.internal_transfers, token, query_id).await
        }
        "api_uniswap_v3_prices" => insert_rows(client, table, &b.v3_prices, token, query_id).await,
        "api_whale_alerts" => insert_rows(client, table, &b.whale_alerts, token, query_id).await,
        "world_contract_interactions" => {
            insert_rows(client, table, &b.contract_interactions, token, query_id).await
        }
        "api_gas_by_function" => {
            insert_rows(client, table, &b.gas_by_function, token, query_id).await
        }
        "api_lending_liquidations" => {
            insert_rows(client, table, &b.lending_liquidations, token, query_id).await
        }
        "world_mev_hourly_stats" => insert_rows(client, table, &b.mev_stats, token, query_id).await,
        "api_token_balances" => {
            insert_rows(client, table, &b.token_balances, token, query_id).await
        }
        "latest_oracle_prices" => {
            insert_rows(client, table, &b.latest_oracle_prices, token, query_id).await
        }
        "api_uniswap_mints" => insert_rows(client, table, &b.uniswap_mints, token, query_id).await,
        "api_morpho_activity" => {
            insert_rows(client, table, &b.morpho_activity, token, query_id).await
        }
        LEDGER_TABLE => insert_rows(client, table, &ledger, token, query_id).await,
        other => Err(InsertFailure {
            message: format!("no batch for table {other}"),
            not_submitted: true,
        }),
    }
}

/// One synchronous insert of one table's rows of one block, under the block's deduplication token: ClickHouse keeps
/// the first insert with this token and drops every later one within the table's window (out/DEDUP-PROOF.md).
/// `deduplicate_insert` (26.8) supersedes insert_deduplicate/async_insert_deduplicate; set per query so no profile
/// change can turn it off.
///
/// The caller records the send before this runs. Empty input sends no request. `insert.write` may transmit a chunk
/// before `Insert::end`; only a typed connect failure from those calls is `not_submitted`.
pub(crate) async fn insert_rows<T: clickhouse::Row + serde::Serialize>(
    client: &Client,
    table: &str,
    rows: &[T],
    token: &str,
    query_id: &str,
) -> Result<(), InsertFailure> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut insert = client
        .insert(table)?
        .with_option("async_insert", "0")
        .with_option("deduplicate_insert", "enable")
        .with_option("insert_deduplication_token", token)
        .with_option("query_id", query_id)
        .with_timeouts(Some(INSERT_SEND_TIMEOUT), Some(INSERT_END_TIMEOUT));
    for row in rows {
        insert.write(row).await?;
    }
    insert.end().await.map_err(InsertFailure::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest_journal::{Cutover, RESEND_HORIZON_MS};
    use crate::mv_matcher::{TOPIC_ORACLE_UPDATE, TOPIC_TRANSFER, TOPIC_V2_SWAP};
    use crate::nodestream::{Log, Trace, Transaction};
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    #[derive(Clone, Copy, Debug)]
    enum Fail {
        /// nothing reached the server
        Refused,
        /// An interrupted delivery with no server record; only a separate cannot-arrive proof permits recovery.
        Undelivered,
        /// the rows landed, then the client saw an error (reset, timeout)
        LandedThenError,
        /// a positive ExceptionBeforeStart record proves that no insert pipeline ran
        Rejected,
        /// the client saw an error while the send was still on its way; it lands at `deliver_hidden` (the review's
        /// dequeued-but-unscheduled async batch, or any send no barrier can see)
        Hidden,
        /// the server recorded the token in its deduplication log, then failed before committing the part (crash or
        /// failed rename, MergeTreeSink::commitPart): no rows, and a re-send under the token is acknowledged and dropped
        Unbacked,
        /// a terminal processing exception may commit a prefix; its NewPart evidence was lost
        ProcessingException,
        /// the rows landed, then the client saw an error, and no log record of the send survived (the review's
        /// "committed insert whose log record was dropped")
        LandedNoRecord,
        /// the rows landed under another query_id (a proxy rewrote it), then the client saw an error; the query_log
        /// record still carries the token
        LandedForeignRecord,
        /// `insert.write` failed before `Insert::end`. Earlier chunks can already have committed, and there is no
        /// terminal record, so the send stays uncertain.
        PrefixWrite,
    }

    /// ClickHouse stand-in with independent commit and exact-query terminal evidence. Deduplicated sends have no
    /// successful NewPart record; missing or rewritten query IDs never acquire a terminal record by token.
    #[derive(Default)]
    struct Fake {
        sends: RefCell<Vec<String>>,
        sent_tokens: RefCell<Vec<(String, String)>>,
        tokens: RefCell<HashSet<(String, String)>>,
        landed: RefCell<HashMap<String, u32>>,
        fail: RefCell<HashMap<String, Vec<Fail>>>,
        hidden: RefCell<Vec<(String, String, String)>>,
        records: RefCell<Vec<(String, String, String, bool)>>,
        terminal_records: RefCell<HashMap<String, bool>>,
        /// rows other writers stored, per table (presence and count)
        foreign: RefCell<HashMap<String, u64>>,
        queue: RefCell<Vec<(u64, u64, u64, String)>>,
        trace_queue: RefCell<Vec<crate::healer::TraceGapRow>>,
        enqueue_failures: RefCell<u32>,
        counts_asked: RefCell<Vec<String>>,
        /// tables whose rows the count can't see (an OWN_ROWS_COUNTED table whose rows carry another entry's ingested_at)
        uncounted: RefCell<HashSet<String>>,
        rebuilt: RefCell<Option<BlockNotification>>,
        /// None models unavailable cannot-arrive evidence; existing lost-log tests remain fail-closed.
        server_fence: RefCell<Option<ServerFence>>,
        query_starts: RefCell<HashSet<String>>,
        processes: RefCell<HashSet<String>>,
        absence_error: RefCell<Option<String>>,
    }

    impl Fake {
        fn fail(&self, table: &str, how: Fail) {
            self.fail
                .borrow_mut()
                .entry(table.to_string())
                .or_default()
                .push(how);
        }
        fn landed(&self, table: &str) -> u32 {
            self.landed.borrow().get(table).copied().unwrap_or(0)
        }
        /// The send's rows land unless its token already did; part_log gets a NewPart record (error 0 when they
        /// landed, 389 when dropped) under `query_id`, unless `query_id` is None (the record is lost).
        fn land(&self, table: &str, token: &str, query_id: Option<&str>) {
            let new = self
                .tokens
                .borrow_mut()
                .insert((table.to_string(), token.to_string()));
            if new {
                *self
                    .landed
                    .borrow_mut()
                    .entry(table.to_string())
                    .or_insert(0) += 1;
            }
            if let Some(q) = query_id {
                self.records.borrow_mut().push((
                    table.to_string(),
                    token.to_string(),
                    q.to_string(),
                    new,
                ));
            }
        }
        fn deliver_hidden(&self) {
            for (t, tok, q) in self.hidden.take() {
                self.land(&t, &tok, Some(&q));
                self.terminal_records.borrow_mut().insert(q, false);
            }
        }
        fn stored(&self, table: &str) -> u64 {
            u64::from(self.landed(table)) + self.foreign.borrow().get(table).copied().unwrap_or(0)
        }
        fn tokens_sent(&self, table: &str) -> Vec<String> {
            self.sent_tokens
                .borrow()
                .iter()
                .filter(|(t, _)| t == table)
                .map(|(_, tok)| tok.clone())
                .collect()
        }
        fn evict(&self, table: &str) {
            self.tokens.borrow_mut().retain(|(t, _)| t != table);
        }
    }

    impl IngestTarget for Fake {
        async fn insert(
            &self,
            _b: &Batches,
            _e: &Entry,
            table: &str,
            token: &str,
            q: &str,
        ) -> Result<(), InsertFailure> {
            self.sends.borrow_mut().push(table.to_string());
            self.sent_tokens
                .borrow_mut()
                .push((table.to_string(), token.to_string()));
            let how = self.fail.borrow_mut().get_mut(table).and_then(|v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v.remove(0))
                }
            });
            match how {
                None => {
                    self.land(table, token, Some(q));
                    self.terminal_records
                        .borrow_mut()
                        .insert(q.to_string(), false);
                    Ok(())
                }
                Some(Fail::Refused) => Err(InsertFailure {
                    message: "connection refused".into(),
                    not_submitted: true,
                }),
                Some(Fail::Undelivered) => Err(InsertFailure {
                    message: "delivery interrupted".into(),
                    not_submitted: false,
                }),
                Some(Fail::LandedThenError) => {
                    self.land(table, token, Some(q));
                    self.terminal_records
                        .borrow_mut()
                        .insert(q.to_string(), false);
                    Err(InsertFailure {
                        message: "connection reset".into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::Rejected) => {
                    self.terminal_records
                        .borrow_mut()
                        .insert(q.to_string(), true);
                    Err(InsertFailure {
                        message: "Code: 252. DB::Exception: Too many parts".into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::Hidden) => {
                    self.hidden.borrow_mut().push((
                        table.to_string(),
                        token.to_string(),
                        q.to_string(),
                    ));
                    Err(InsertFailure {
                        message: "timeout".into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::Unbacked) => {
                    self.tokens
                        .borrow_mut()
                        .insert((table.to_string(), token.to_string()));
                    self.records.borrow_mut().push((
                        table.to_string(),
                        token.to_string(),
                        q.to_string(),
                        false,
                    ));
                    self.terminal_records
                        .borrow_mut()
                        .insert(q.to_string(), false);
                    Err(InsertFailure {
                        message: "Code: 1001. DB::Exception: filesystem error (rename of the part)"
                            .into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::ProcessingException) => {
                    self.land(table, token, None);
                    self.terminal_records
                        .borrow_mut()
                        .insert(q.to_string(), false);
                    Err(InsertFailure {
                        message: "Code: 252. DB::Exception: insert failed while processing".into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::LandedNoRecord) => {
                    self.land(table, token, None);
                    Err(InsertFailure {
                        message: "connection reset".into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::LandedForeignRecord) => {
                    self.land(table, token, Some("foreign-query-id"));
                    self.terminal_records
                        .borrow_mut()
                        .insert("foreign-query-id".into(), false);
                    Err(InsertFailure {
                        message: "connection reset".into(),
                        not_submitted: false,
                    })
                }
                Some(Fail::PrefixWrite) => Err(InsertFailure {
                    message: "insert write failed before the body ended".into(),
                    not_submitted: false,
                }),
            }
        }
        async fn presence(
            &self,
            _e: &Entry,
            tables: &[String],
        ) -> Result<Vec<(String, u64)>, String> {
            Ok(tables.iter().map(|t| (t.clone(), self.stored(t))).collect())
        }
        async fn count(&self, sql: &str) -> Result<u64, String> {
            let table = sql
                .split("worldchain.")
                .nth(1)
                .and_then(|s| s.split(' ').next())
                .unwrap()
                .to_string();
            self.counts_asked.borrow_mut().push(table.clone());
            Ok(if self.uncounted.borrow().contains(&table) {
                0
            } else {
                self.stored(&table)
            })
        }
        async fn enqueue_gap(&self, lo: u64, hi: u64, at: u64, source: &str) -> Result<(), String> {
            let mut f = self.enqueue_failures.borrow_mut();
            if *f > 0 {
                *f -= 1;
                return Err("Code: 252".into());
            }
            self.queue
                .borrow_mut()
                .push((lo, hi, at, source.to_string()));
            Ok(())
        }
        async fn enqueue_trace_gap(&self, row: &crate::healer::TraceGapRow) -> Result<(), String> {
            self.trace_queue.borrow_mut().push(row.clone());
            Ok(())
        }
        async fn stored_hash(&self, _block: u64) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn committed(
            &self,
            table: &str,
            token: &str,
            query_ids: &[String],
            _since_ms: u64,
        ) -> Result<bool, String> {
            Ok(self.records.borrow().iter().any(|(t, tok, q, ok)| {
                *ok && t == table && (query_ids.contains(q) || tok == token)
            }))
        }
        async fn terminal(
            &self,
            query_ids: &[String],
            _since_ms: u64,
        ) -> Result<Vec<TerminalSend>, String> {
            let records = self.terminal_records.borrow();
            Ok(query_ids
                .iter()
                .filter_map(|query_id| {
                    records.get(query_id).map(|rejected| TerminalSend {
                        query_id: query_id.clone(),
                        rejected: *rejected,
                    })
                })
                .collect())
        }
        async fn absent_sends(&self, t: &TableState) -> Result<AbsentSends, String> {
            if let Some(error) = self.absence_error.borrow().as_ref() {
                return Err(error.clone());
            }
            let Some(server) = *self.server_fence.borrow() else {
                return Ok(AbsentSends::default());
            };
            let mut proof = absence_candidates(t, server);
            proof.query_ids.retain(|id| {
                !self.query_starts.borrow().contains(id)
                    && !self.processes.borrow().contains(id)
                    && !self.terminal_records.borrow().contains_key(id)
            });
            Ok(proof)
        }
        async fn stored_block(&self, _block: u64) -> Result<Option<BlockNotification>, String> {
            Ok(self.rebuilt.borrow().clone())
        }
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            let root = std::env::temp_dir().join(format!(
                "tank-c1v2-streamer-{}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::SeqCst),
                ingest_journal::wall_ms()
            ));
            fs::create_dir_all(root.join("pending")).unwrap();
            fs::write(
                root.join("cutover.json"),
                br#"{"version":1,"floor":10,"complete_tip":9,"created_at_ms":1}"#,
            )
            .unwrap();
            Dir(root)
        }
        /// The fence wait is over at once: an unfenced send parks at its first unfenced attempt.
        fn ingest(&self) -> Ingest {
            self.ingest_waiting(0)
        }
        fn ingest_waiting(&self, fence_wait_ms: u64) -> Ingest {
            let cutover = self.0.join("cutover.json");
            let c = Cutover::load(&cutover).unwrap();
            Ingest::new(
                Journal::open(&self.0.join("pending"), &self.0.join("writer.lock"))
                    .unwrap()
                    .with_fence_wait_ms(fence_wait_ms),
                cutover,
                &c,
            )
        }
        fn files(&self) -> Vec<String> {
            let mut v: Vec<String> = fs::read_dir(self.0.join("pending"))
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            v.sort();
            v
        }
        fn entry(&self, i: &Ingest) -> Entry {
            match i.journal.load(EntryKind::Stream).unwrap().pop() {
                Some(Loaded::Entry(e)) => e,
                other => panic!("expected one entry, got {other:?}"),
            }
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn word(n: u64) -> String {
        format!("0x{n:064x}")
    }

    /// One contract call with an ERC-20 Transfer log, a swap pair (MEV row) and a value-carrying call trace.
    fn block(n: u64) -> BlockNotification {
        let token = "0x00000000000000000000000000000000000000aa".to_string();
        let swap = |i: u32| Log {
            tx_hash: word(1),
            log_index: i,
            address: "0x00000000000000000000000000000000000000dd".into(),
            topics: vec![TOPIC_V2_SWAP.to_string()],
            data: word(9),
            ..Default::default()
        };
        BlockNotification {
            number: n,
            hash: word(n),
            parent_hash: word(n - 1),
            timestamp: 1_760_000_000 + 2 * n,
            miner: "0x4200000000000000000000000000000000000011".into(),
            tx_count: 1,
            txs: vec![Transaction {
                hash: word(1),
                from: "0x00000000000000000000000000000000000000bb".into(),
                to: token.clone(),
                value: "0".into(),
                input: "0xa9059cbb00000000".into(),
                status: 1,
                l1_fee: "7".into(),
                mint_wei: "0".into(),
                ..Default::default()
            }],
            traces: vec![Trace {
                tx_hash: word(1),
                from_address: "0x00000000000000000000000000000000000000bb".into(),
                to_address: "0x00000000000000000000000000000000000000cc".into(),
                value_wei: "5".into(),
                trace_type: "call".into(),
                call_type: "call".into(),
                ..Default::default()
            }],
            logs: vec![
                Log {
                    tx_hash: word(1),
                    address: token,
                    topics: vec![TOPIC_TRANSFER.to_string(), word(0xbb), word(0xcc)],
                    data: word(3),
                    ..Default::default()
                },
                swap(1),
                swap(2),
                Log {
                    // WLD oracle update: api_oracle_prices and latest_oracle_prices rows
                    tx_hash: word(1),
                    log_index: 3,
                    address: "0x8bb2943ab030e3ee05a58d9832525b4f60a97fa0".into(),
                    topics: vec![
                        TOPIC_ORACLE_UPDATE.to_string(),
                        word(2_000_000_000_000_000_000),
                    ],
                    data: word(0),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn tables_of(kind: EntryKind, n: u64) -> Vec<&'static str> {
        build_batches(&block(n), 1)
            .tables(kind)
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    fn assert_once(fake: &Fake, kind: EntryKind, n: u64) {
        for t in tables_of(kind, n) {
            assert_eq!(
                fake.landed(t),
                1,
                "{t} must hold the block's rows exactly once"
            );
        }
    }

    fn trace_hole_block(n: u64, partial: bool) -> BlockNotification {
        let mut b = block(n);
        if partial {
            b.tx_count = 2;
            b.txs.push(Transaction {
                tx_index: 1,
                hash: word(2),
                ..b.txs[0].clone()
            });
        } else {
            b.traces.clear();
        }
        b
    }

    fn save_stream_entry(ingest: &Ingest, block: &BlockNotification) -> Entry {
        let batches = build_batches(block, 1);
        let mut entry = Entry::new(
            EntryKind::Stream,
            block.number,
            &block.hash,
            block.timestamp,
            1,
            &batches.tables(EntryKind::Stream),
        );
        ingest.journal.create(&mut entry, &block.encode_to_vec()).unwrap();
        entry
    }

    struct LocalSubscription {
        start_block: u64,
    }

    impl tonic::server::ServerStreamingService<SubscribeRequest> for LocalSubscription {
        type Response = BlockNotification;
        type ResponseStream = tonic::codegen::tokio_stream::Iter<
            std::vec::IntoIter<Result<BlockNotification, tonic::Status>>,
        >;
        type Future = std::future::Ready<
            Result<tonic::Response<Self::ResponseStream>, tonic::Status>,
        >;

        fn call(&mut self, request: tonic::Request<SubscribeRequest>) -> Self::Future {
            assert_eq!(request.into_inner().start_block, self.start_block);
            std::future::ready(Ok(tonic::Response::new(
                tonic::codegen::tokio_stream::iter(vec![Ok(block(self.start_block))]),
            )))
        }
    }

    /// Exercise the generated client and real gRPC framing without a node socket or a generated server.
    async fn subscribe_locally(start_block: u64) -> BlockNotification {
        let service = service_fn(move |request: tonic::codegen::http::Request<tonic::body::BoxBody>| async move {
            assert_eq!(request.uri().path(), "/nodestream.NodeStream/Subscribe");
            let mut grpc = tonic::server::Grpc::new(
                tonic::codec::ProstCodec::<BlockNotification, SubscribeRequest>::default(),
            );
            Ok::<_, std::convert::Infallible>(
                grpc.server_streaming(LocalSubscription { start_block }, request).await,
            )
        });
        let mut subscriber = NodeStreamClient::new(service);
        let mut stream = subscriber
            .subscribe(SubscribeRequest { start_block })
            .await
            .unwrap()
            .into_inner();
        let received = stream.message().await.unwrap().unwrap();
        assert_eq!(received.number, start_block);
        assert!(stream.message().await.unwrap().is_none());
        received
    }

    #[test]
    fn stream_trace_coverage_handles_empty_full_and_missing_lists() {
        assert!(!stream_missing_traces(&BlockNotification::default()));
        assert!(!stream_missing_traces(&block(10)));
        assert!(stream_missing_traces(&trace_hole_block(10, false)));
        assert!(stream_missing_traces(&trace_hole_block(10, true)));
        let mut b = block(10);
        b.txs.clear();
        assert!(!stream_missing_traces(&b), "no transaction needs a trace");
    }

    #[test]
    fn stream_trace_coverage_uses_indices_not_order_or_trace_count() {
        let mut b = trace_hole_block(10, true);
        b.tx_count = 0; // the received transaction indices, not the count field, define coverage
        b.traces.push(b.traces[0].clone());
        assert_eq!(b.traces.len(), b.txs.len());
        assert!(stream_missing_traces(&b), "duplicate traces do not cover index 1");
        b.traces.push(Trace {
            tx_index: 9,
            ..b.traces[0].clone()
        });
        assert!(stream_missing_traces(&b), "an unrelated index does not cover index 1");
        b.traces.insert(0, Trace {
            tx_index: 1,
            ..b.traces[0].clone()
        });
        assert!(!stream_missing_traces(&b), "unordered full coverage is sufficient");
        b.txs.reverse();
        assert!(!stream_missing_traces(&b));
    }

    #[tokio::test]
    async fn missing_stream_traces_claim_one_height_without_sends_and_allow_the_next() {
        for partial in [false, true] {
            let d = Dir::new();
            let i = d.ingest();
            let missing = Fake::default();
            let mut pos = StreamPos {
                expected: 10,
                first: false,
                cutover: None,
            };
            let floor = i.floor;
            assert!(!handle_block(&missing, &i, &mut pos, &trace_hole_block(10, partial))
                .await
                .unwrap());
            let claims = i.journal.claims().unwrap();
            assert_eq!(claims.len(), 1);
            assert_eq!((claims[0].lo, claims[0].hi), (10, 10));
            assert_eq!(claims[0].source, "stream-missing-traces");
            assert_eq!(d.files(), vec!["gap-10-10.json"]);
            assert!(!i.journal.owns(10));
            assert!(missing.sends.borrow().is_empty());
            assert_eq!(missing.landed(LEDGER_TABLE), 0);
            assert_eq!(pos.expected, 11);
            assert_eq!(i.stream_expected.load(Ordering::SeqCst), 11);
            assert_eq!(i.floor, floor);
            // Fake presence is table-global, so each height gets its own target.
            let next = Fake::default();
            assert!(handle_block(&next, &i, &mut pos, &block(11)).await.unwrap());
            assert_once(&next, EntryKind::Stream, 11);
            assert_eq!(pos.expected, 12);
            assert_eq!(i.stream_expected.load(Ordering::SeqCst), 12);
            assert_eq!(i.floor, floor);
            assert_eq!(i.journal.claims().unwrap(), claims);
            assert_eq!(d.files(), vec!["gap-10-10.json"]);
        }
    }

    #[tokio::test]
    async fn fully_traced_and_zero_transaction_stream_blocks_complete_normally() {
        for empty in [false, true] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            let mut b = block(10);
            if empty {
                b.tx_count = 0;
                b.txs.clear();
                b.traces.clear();
                b.logs.clear();
            }
            let tables = build_batches(&b, 1).tables(EntryKind::Stream);
            assert!(ingest_new_block(&fake, &i, &b).await.unwrap());
            assert_eq!(
                *fake.sends.borrow(),
                tables.iter().map(|(t, _)| *t).collect::<Vec<_>>()
            );
            for (table, _) in tables {
                assert_eq!(fake.landed(table), 1, "{table} must be written once");
            }
            assert_eq!(fake.landed(LEDGER_TABLE), 1);
            assert!(i.journal.claims().unwrap().is_empty());
            assert!(d.files().is_empty());
        }
    }

    #[tokio::test]
    async fn missing_trace_claim_survives_restart_without_duplicate_sends() {
        let d = Dir::new();
        let i = d.ingest();
        let missing = Fake::default();
        let b = trace_hole_block(10, false);
        assert!(!ingest_new_block(&missing, &i, &b).await.unwrap());
        let claims = i.journal.claims().unwrap();
        drop(i); // crash after the durable claim, before n+1 arrives

        let i = d.ingest();
        assert_eq!(resume_stream_entries(&missing, &i).await.unwrap(), 0);
        assert_eq!(i.journal.claims().unwrap(), claims);
        let mut pos = StreamPos {
            expected: 10, // with no transaction rows from n, a reconnect may receive n again
            first: false,
            cutover: None,
        };
        assert!(!handle_block(&missing, &i, &mut pos, &b).await.unwrap());
        assert_eq!(i.journal.claims().unwrap(), claims);
        assert!(missing.sends.borrow().is_empty());
        assert_eq!(missing.landed(LEDGER_TABLE), 0);
        assert!(!i.journal.owns(10));
        // Separate target: Fake's stored-table presence is not height-scoped.
        let next = Fake::default();
        assert!(handle_block(&next, &i, &mut pos, &block(11)).await.unwrap());
        assert_once(&next, EntryKind::Stream, 11);
        let sent = next.sends.borrow().clone();
        assert!(!handle_block(&next, &i, &mut pos, &block(11)).await.unwrap());
        assert_eq!(*next.sends.borrow(), sent);
        process_gap_claims(&missing, &i.journal).await.unwrap();
        process_gap_claims(&missing, &i.journal).await.unwrap();
        assert_eq!(missing.queue.borrow().len(), 1);
        assert!(missing.sends.borrow().is_empty());
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn retrying_trace_gap_claim_preserves_acknowledged_chunk_progress() {
        for (lo, hi) in [(10, 10), (20, 20_020)] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            let mut claim = persist_gap_claim(&i.journal, lo, hi, "stream-missing-traces").await;
            let first = claim.chunks()[0];
            claim.enqueued.push([first.0, first.1]);
            i.journal.save_claim(&claim).unwrap();
            let pending = claim.pending_chunks();
            let retried = persist_gap_claim(&i.journal, lo, hi, "stream-missing-traces").await;
            assert_eq!(retried, claim);
            if lo == hi {
                assert!(!ingest_new_block(&fake, &i, &trace_hole_block(lo, false))
                    .await
                    .unwrap());
                assert_eq!(i.journal.claims().unwrap(), vec![claim]);
            }
            process_gap_claims(&fake, &i.journal).await.unwrap();
            assert_eq!(
                fake.queue.borrow().iter().map(|q| (q.0, q.1)).collect::<Vec<_>>(),
                pending
            );
            assert!(fake.sends.borrow().is_empty());
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
            assert!(d.files().is_empty());
        }
    }

    #[tokio::test]
    async fn unsent_missing_trace_entries_and_orphans_retire_to_a_gap_on_resume() {
        for orphan in [false, true] {
            for partial in [false, true] {
                let d = Dir::new();
                let i = d.ingest();
                let fake = Fake::default();
                let entry = save_stream_entry(&i, &trace_hole_block(10, partial));
                assert!(entry.tables.iter().all(|t| t.sends == 0));
                if orphan {
                    fs::remove_file(d.0.join("pending/10.json")).unwrap();
                }
                drop(i);

                let i = d.ingest();
                assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 10);
                assert_eq!(d.files(), vec!["gap-10-10.json"]);
                assert!(!i.journal.owns(10));
                assert!(fake.sends.borrow().is_empty());
                assert_eq!(fake.landed(LEDGER_TABLE), 0);
                let claims = i.journal.claims().unwrap();
                assert_eq!(claims.len(), 1);
                assert_eq!((claims[0].lo, claims[0].hi), (10, 10));
                assert_eq!(claims[0].source, "stream-missing-traces");
                assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 0);
                assert_eq!(i.journal.claims().unwrap(), claims);
            }
        }
    }

    #[tokio::test]
    async fn retired_unsent_state_is_saved_before_finish_and_cleans_up_after_restart() {
        for payload_removed in [false, true] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            let mut entry = save_stream_entry(&i, &trace_hole_block(10, true));
            let path = d.0.join("pending/10.block");
            let payload = fs::read(&path).unwrap();
            // Make finish fail after the retirement save, then model both payload-first crash points.
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            {
                let _owner = i.locks.lock(10).await;
                let err = retire_unsent_stream_to_gap(&i, &mut entry).await.unwrap_err();
                assert!(err.contains("retirement finish failed"), "{err}");
            }
            let saved = d.entry(&i);
            assert!(saved.is_done());
            assert!(saved.tables.iter().all(|t| t.sends == 0 && t.sends_fenced()));
            assert!(!saved.tables.iter().any(|t| t.first_send_ms.is_some()));
            let claims = i.journal.claims().unwrap();
            fs::remove_dir(&path).unwrap();
            if !payload_removed {
                fs::write(&path, payload).unwrap();
            }
            drop(i);

            let i = d.ingest();
            assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 10);
            assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 0);
            assert_eq!(i.journal.claims().unwrap(), claims);
            assert_eq!(d.files(), vec!["gap-10-10.json"]);
            assert!(!i.journal.owns(10));
            assert!(fake.sends.borrow().is_empty());
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
            process_gap_claims(&fake, &i.journal).await.unwrap();
            process_gap_claims(&fake, &i.journal).await.unwrap();
            assert_eq!(fake.queue.borrow().len(), 1);
            assert!(d.files().is_empty());
        }
    }

    #[tokio::test]
    async fn previously_sent_missing_trace_owner_resumes_then_accepts_next_and_subscribes() {
        for partial in [false, true] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            let b = trace_hole_block(10, partial);
            let mut entry = save_stream_entry(&i, &b);
            let table = entry.tables[0].table.clone();
            entry.tables[0].generation = 1;
            let query_id = ingest_journal::query_id(10, &table, 1);
            entry.tables[0].record_send(&query_id, 1).unwrap();
            i.journal.save(&mut entry).unwrap();
            let token = ingest_journal::generation_token(
                &ingest_journal::dedup_token(&table, 10, &b.hash),
                1,
            );
            fake.insert(&build_batches(&b, 1), &entry, &table, &token, &query_id)
                .await
                .unwrap();
            let payload = fs::read(d.0.join("pending/10.block")).unwrap();
            drop(i);

            let i = d.ingest();
            let floor = i.floor;
            // A real later insert error still propagates, after the old owner is resumed without replay.
            fake.fail(LEDGER_TABLE, Fail::Refused);
            let err = resume_stream_entries(&fake, &i).await.unwrap_err();
            assert!(err.contains(&format!("insert {LEDGER_TABLE} failed")), "{err}");
            let saved = d.entry(&i);
            assert_eq!(
                saved.tables.iter().map(|t| (&t.table, t.rows, t.generation)).collect::<Vec<_>>(),
                entry.tables.iter().map(|t| (&t.table, t.rows, t.generation)).collect::<Vec<_>>()
            );
            let sent = &saved.tables[0];
            let before = &entry.tables[0];
            assert_eq!(sent.status, TableStatus::Done);
            assert_eq!(sent.sends, before.sends);
            assert_eq!(sent.first_send_ms, before.first_send_ms);
            assert_eq!(sent.query_ids, before.query_ids);
            assert_eq!(sent.clients, before.clients);
            assert_eq!(sent.rejected_query_ids, before.rejected_query_ids);
            assert!(sent.terminal_query_ids.contains(&query_id));
            assert_eq!(fs::read(d.0.join("pending/10.block")).unwrap(), payload);

            assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 10);
            assert!(!i.journal.owns(10));
            assert!(d.files().is_empty());
            assert!(i.journal.claims().unwrap().is_empty(), "a sent owner is not retired");
            assert_eq!(fake.tokens_sent(&table), vec![token]);
            assert_eq!(fake.landed(&table), 1);
            assert_eq!(fake.landed(LEDGER_TABLE), 1);
            assert_eq!(fake.trace_queue.borrow().len(), 2);
            assert!(fake.trace_queue.borrow().iter().all(|row| {
                row.block_number == 10
                    && row.reason == "stream-missing-traces"
                    && row.rpc_error_code == 0
                    && row.resolved == 0
                    && row.resolved_at == 0
            }));
            let sends = fake.sends.borrow().clone();
            assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 0);
            assert_eq!(*fake.sends.borrow(), sends);
            let mut pos = StreamPos {
                expected: 11,
                first: false,
                cutover: None,
            };
            // Fake presence is table-global, so each later height gets a separate target.
            let next = Fake::default();
            assert!(handle_block(&next, &i, &mut pos, &block(11)).await.unwrap());
            assert_once(&next, EntryKind::Stream, 11);
            let streamed = subscribe_locally(pos.expected).await;
            let subscribed = Fake::default();
            assert!(handle_block(&subscribed, &i, &mut pos, &streamed).await.unwrap());
            assert_once(&subscribed, EntryKind::Stream, 12);
            assert_eq!(i.stream_expected.load(Ordering::SeqCst), 13);
            assert_eq!(i.floor, floor);
        }
    }

    #[test]
    fn unsent_stream_predicate_checks_every_send_history_field() {
        let entry = Entry::new(EntryKind::Stream, 10, &word(10), 1, 1, &[("world_transactions", 1)]);
        assert!(stream_entry_is_unsent(&entry));
        let mut sent = entry.clone();
        sent.tables[0].record_send("legacy", 1).unwrap();
        for history in ["sends", "generation", "first_send", "query", "clients", "terminal", "rejected"] {
            // Individual residual fields need not form a replayable journal entry; none proves UNSENT.
            let mut residual = entry.clone();
            let table = &mut residual.tables[0];
            match history {
                "sends" => table.sends = 1,
                "generation" => table.generation = 1,
                "first_send" => table.first_send_ms = Some(1),
                "query" => table.query_ids.push("legacy".into()),
                "clients" => table.clients = sent.tables[0].clients.clone(),
                "terminal" => table.terminal_query_ids.push("legacy".into()),
                "rejected" => table.rejected_query_ids.push("legacy".into()),
                _ => unreachable!(),
            }
            assert!(!stream_entry_is_unsent(&residual), "{history}");
        }
    }

    #[tokio::test]
    async fn missing_trace_owner_with_residual_generation_completes_without_retirement() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let b = trace_hole_block(10, false);
        let mut entry = save_stream_entry(&i, &b);
        let table = entry.tables[0].table.clone();
        entry.tables[0].record_send("legacy-rejected", 1).unwrap();
        entry.tables[0].record_terminal("legacy-rejected", true);
        assert_eq!(entry.tables[0].next_generation().unwrap(), 1);
        assert_eq!(entry.tables[0].sends, 0);
        assert!(entry.tables[0].query_ids.is_empty());
        assert!(entry.tables[0].clients.is_empty());
        assert!(!stream_entry_is_unsent(&entry));
        i.journal.save(&mut entry).unwrap();
        drop(i);

        let i = d.ingest();
        assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 10);
        assert!(!i.journal.owns(10));
        assert!(i.journal.claims().unwrap().is_empty());
        assert!(d.files().is_empty());
        assert_eq!(fake.trace_queue.borrow().len(), 1);
        assert_eq!(fake.trace_queue.borrow()[0].block_number, 10);
        assert_eq!(fake.landed(LEDGER_TABLE), 1);
        assert_eq!(fake.tokens_sent(&table), vec![ingest_journal::generation_token(
            &ingest_journal::dedup_token(&table, 10, &b.hash),
            1,
        )]);
        let mut pos = StreamPos { expected: 11, first: false, cutover: None };
        let next = Fake::default();
        assert!(handle_block(&next, &i, &mut pos, &block(11)).await.unwrap());
        let streamed = subscribe_locally(pos.expected).await;
        assert_eq!(streamed.number, 12);
    }

    #[tokio::test]
    async fn reconstructed_stream_with_missing_transaction_traces_completes_existing_owner_once() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Arc::new(std::sync::Mutex::new(Fake::default()));
        let partial = trace_hole_block(10, true);
        let batches = build_batches(&partial, 1);
        let mut entry = save_stream_entry(&i, &partial);
        let sent_table = "world_contract_interactions";
        let sent_index = entry.tables.iter().position(|t| t.table == sent_table).unwrap();
        entry.tables[sent_index].generation = 1;
        let query_id = ingest_journal::query_id(10, sent_table, 1);
        entry.tables[sent_index].record_send(&query_id, 1).unwrap();
        i.journal.save(&mut entry).unwrap();
        let token = ingest_journal::generation_token(
            &ingest_journal::dedup_token(sent_table, 10, &partial.hash),
            1,
        );
        {
            let fake = fake.lock().unwrap();
            fake.insert(&batches, &entry, sent_table, &token, &query_id).await.unwrap();
            *fake.rebuilt.borrow_mut() = Some(partial.clone());
            for (table, rows) in [
                ("world_blocks", 1),
                ("world_transactions", partial.txs.len() as u64),
                ("world_logs", partial.logs.len() as u64),
                ("world_traces", partial.traces.len() as u64),
            ] {
                fake.foreign.borrow_mut().insert(table.into(), rows);
            }
        }
        fs::remove_file(d.0.join("pending/10.block")).unwrap();
        drop(i);

        let i = d.ingest();
        let sends = {
            let fake = fake.lock().unwrap();
            assert_eq!(resume_stream_entries(&*fake, &i).await.unwrap(), 10);
            assert!(!i.journal.owns(10));
            assert!(i.journal.claims().unwrap().is_empty());
            assert!(d.files().is_empty());
            assert_eq!(fake.rebuilt.borrow().as_ref(), Some(&partial));
            let sends = fake.sends.borrow().clone();
            // One ledger send and one landing per derived table observe a single completion, not dedup-dropped retries.
            for (table, _) in batches.tables(EntryKind::Stream) {
                let expected = if table_class(table) == TableClass::Core { 0 } else { 1 };
                assert_eq!(fake.landed(table), expected, "{table} landed more than once");
                assert_eq!(
                    sends.iter().filter(|sent| sent.as_str() == table).count(),
                    expected as usize,
                    "{table} was sent more than once"
                );
            }
            assert_eq!(fake.landed(LEDGER_TABLE), 1);
            assert_eq!(fake.tokens_sent(sent_table), vec![token]);
            assert_eq!(fake.stored("world_traces"), 1);
            let gaps = fake.trace_queue.borrow();
            assert_eq!(gaps.len(), 1);
            assert_eq!(gaps[0].block_number, 10);
            assert_eq!(
                gaps[0].reason,
                "stream-missing-traces: rebuilt block 10 is missing transaction traces"
            );
            assert_eq!(gaps[0].rpc_error_code, 0);
            assert_eq!(gaps[0].resolved, 0);
            assert_eq!(gaps[0].resolved_at, 0);
            sends
        };

        let mock = crate::raw_healer::tests::RawMock::new(true).await;
        let height = partial.number;
        let tx_count = u64::from(partial.tx_count);
        let queries = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let observed_queries = queries.clone();
        let stored_fake = fake.clone();
        mock.state.lock().unwrap().query_override = Some(Box::new(move |query, _body| {
            observed_queries.lock().unwrap().push(query.to_string());
            // Read the actual stored effects, never seed a successful ledger or upgrade the partial rebuild.
            let fake = stored_fake.lock().unwrap();
            if query.contains(" AS tx_count, ") {
                return Some([
                    fake.stored("world_blocks"),
                    tx_count,
                    fake.stored("world_transactions"),
                    fake.stored("world_logs"),
                    fake.stored("world_traces"),
                ].into_iter().flat_map(u64::to_le_bytes).collect());
            }
            if query.starts_with("SELECT toUInt64(ifNull(max(block_number), 0)) AS b")
                && query.contains(LEDGER_TABLE)
            {
                let tip = if fake.landed(LEDGER_TABLE) > 0 { height } else { 0 };
                return Some(tip.to_le_bytes().to_vec());
            }
            if query.starts_with("SELECT number AS b FROM numbers(")
                && query.contains(LEDGER_TABLE)
            {
                assert!(query.contains(&format!("numbers({height}, 1)")), "{query}");
                return Some(if fake.landed(LEDGER_TABLE) > 0 {
                    Vec::new()
                } else {
                    height.to_le_bytes().to_vec()
                });
            }
            None
        }));
        i.stream_expected.store(11, Ordering::SeqCst);
        let target = ClickHouseTarget::new(mock.ch.clone());
        let mut deriver = crate::backfiller::GapDeriver::new(i.floor);
        for _ in 0..2 {
            // No Fake guard is held across HTTP: the loopback response reads that same Fake.
            assert_eq!(
                crate::raw_healer::heal_block(&mock.ch, &mock.http, 10, &i.journal, &i.locks)
                    .await
                    .unwrap(),
                crate::raw_healer::HealOutcome::Healed
            );
            deriver.tick(&mock.ch, &target, &i).await.unwrap();
            let fake = fake.lock().unwrap();
            assert_eq!(resume_stream_entries(&*fake, &i).await.unwrap(), 0);
            assert_eq!(fake.rebuilt.borrow().as_ref(), Some(&partial));
            assert!(!i.journal.owns(10));
            assert!(i.journal.claims().unwrap().is_empty());
            assert!(d.files().is_empty());
            assert_eq!(fake.landed(LEDGER_TABLE), 1);
            assert_eq!(*fake.sends.borrow(), sends);
            assert_eq!(fake.trace_queue.borrow().len(), 1);
            assert_eq!(fake.stored("world_traces"), 1);
        }
        let queries = queries.lock().unwrap();
        assert_eq!(queries.iter().filter(|query| query.contains(" AS tx_count, ")).count(), 2);
        assert_eq!(
            queries.iter().filter(|query| {
                query.starts_with("SELECT number AS b FROM numbers(") && query.contains(LEDGER_TABLE)
            }).count(),
            1,
            "the real deriver must scan for heights missing their ledger row"
        );
        assert!(!queries.iter().any(|query| query.starts_with("SELECT block_number AS b FROM")));
        assert!(!queries.iter().any(|query| query.starts_with("INSERT INTO")));
        let state = mock.state.lock().unwrap();
        assert_eq!(state.trace_inserts, 0);
        assert!(state.rpc_methods.is_empty(), "stored partial traces must not become a complete RPC rebuild");
    }

    #[test]
    fn write_order_core_then_derived_then_ledger() {
        let t = tables_of(EntryKind::Stream, 10);
        assert_eq!(
            &t[..4],
            &[
                "world_transactions",
                "world_logs",
                "world_traces",
                "world_blocks"
            ]
        );
        assert_eq!(*t.last().unwrap(), LEDGER_TABLE);
        for u in [
            "world_contract_interactions",
            "api_token_balances",
            "world_mev_hourly_stats",
            "api_internal_transfers",
        ] {
            assert!(t.contains(&u), "{u} in {t:?}");
        }
        let d = tables_of(EntryKind::Derive, 10);
        assert!(
            d.iter().all(|t| table_class(t) != TableClass::Core),
            "a derive entry writes no core table: {d:?}"
        );
        assert_eq!(d.len(), t.len() - 4);
    }

    #[test]
    fn schema_check_needs_every_window_and_the_ledger() {
        let ok = "ReplacingMergeTree ORDER BY x SETTINGS index_granularity = 8192, non_replicated_deduplication_window = 100000";
        assert_eq!(dedup_window(ok), 100_000);
        assert_eq!(
            dedup_window("MergeTree ORDER BY x SETTINGS index_granularity = 8192"),
            0
        );
        assert!(schema_problems(&[("world_blocks", ok), (LEDGER_TABLE, ok)]).is_ok());
        let e = schema_problems(&[("world_blocks", ok)]).unwrap_err();
        assert!(e.contains("tank_ingest_ledger missing"), "{e}");
        let small = "MergeTree SETTINGS non_replicated_deduplication_window = 1000";
        let e = schema_problems(&[("api_token_balances", small), (LEDGER_TABLE, ok)]).unwrap_err();
        assert!(
            e.contains("api_token_balances: non_replicated_deduplication_window 1000"),
            "{e}"
        );
    }

    #[tokio::test]
    async fn fresh_block_writes_each_table_once_and_leaves_no_entry() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        assert!(ingest_new_block(&fake, &i, &block(10)).await.unwrap());
        assert_eq!(*fake.sends.borrow(), tables_of(EntryKind::Stream, 10));
        assert_once(&fake, EntryKind::Stream, 10);
        assert!(d.files().is_empty());
    }

    /// A delayed request can outlive any deduplication window. Even a globally keyed zero count cannot fence it:
    /// once the fence wait is over (0 here) every table parks without a replay, then exact terminal evidence permits
    /// the operator to confirm late rows.
    #[tokio::test]
    async fn hidden_send_parks_every_table_until_late_arrival_is_fenced_and_confirmed() {
        for table in [
            "world_contract_interactions",
            "world_mev_hourly_stats",
            "world_token_transfers",
            "world_traces",
            LEDGER_TABLE,
        ] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            fake.fail(table, Fail::Hidden);
            assert!(ingest_new_block(&fake, &i, &block(30)).await.is_err());
            resume_stream_entries(&fake, &i).await.unwrap();
            assert!(d.entry(&i).parked().contains(&table));
            assert_eq!(fake.tokens_sent(table).len(), 1, "{table}: never replayed");
            assert_eq!(fake.landed(table), 0);
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
            assert_eq!(d.files(), vec!["30.block", "30.json"]);
            fake.evict(table);
            fake.deliver_hidden();
            resume_stream_entries(&fake, &i).await.unwrap();
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!(
                t.status,
                TableStatus::Parked,
                "late arrival is not operator approval"
            );
            assert!(t.sends_fenced());
            assert_eq!(fake.tokens_sent(table).len(), 1);
            drop(i);
            ingest_journal::resolve(
                &d.0.join("pending"),
                &d.0.join("writer.lock"),
                30,
                table,
                ingest_journal::Resolution::Done,
            )
            .unwrap();
            let i = d.ingest();
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_once(&fake, EntryKind::Stream, 30);
            assert!(d.files().is_empty(), "{table}");
        }
    }

    /// F2 without an operator: within the fence wait an unfenced send is neither parked nor replayed and its generation
    /// stays. Each attempt fails before any send, the entry stays owned, and once the late request terminates (here it
    /// lands, after its token left the window) the next attempt counts its rows and finishes the block exactly once.
    #[tokio::test]
    async fn hidden_send_within_the_fence_wait_is_kept_unsent_and_finishes_once_it_lands() {
        for table in [
            "world_contract_interactions",
            "world_mev_hourly_stats",
            "world_token_transfers",
            "world_traces",
            LEDGER_TABLE,
        ] {
            let d = Dir::new();
            let i = d.ingest_waiting(600_000);
            let fake = Fake::default();
            fake.fail(table, Fail::Hidden);
            assert!(ingest_new_block(&fake, &i, &block(31)).await.is_err());
            for _ in 0..3 {
                let err = resume_stream_entries(&fake, &i).await.unwrap_err();
                assert!(err.contains("INGEST_FENCE_WAIT"), "{table}: {err}");
            }
            let e = d.entry(&i);
            assert!(e.parked().is_empty(), "{table}: waiting is not parking");
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!(t.status, TableStatus::Pending, "{table}");
            assert_eq!(
                (t.sends, t.generation, t.query_ids.len()),
                (1, 0, 1),
                "{table}"
            );
            assert!(!t.sends_fenced(), "{table}");
            assert_eq!(
                fake.tokens_sent(table).len(),
                1,
                "{table}: nothing sent while waiting"
            );
            assert_eq!(fake.landed(table), 0);
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
            fake.evict(table);
            fake.deliver_hidden();
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_eq!(
                fake.tokens_sent(table).len(),
                1,
                "{table}: the late send is the only one"
            );
            assert_once(&fake, EntryKind::Stream, 31);
            assert!(d.files().is_empty(), "{table}");
        }
    }

    #[tokio::test]
    async fn landed_send_whose_answer_was_lost_is_not_counted_twice() {
        for table in [
            "api_token_balances",
            "latest_oracle_prices",
            "api_internal_transfers",
            "world_blocks",
            LEDGER_TABLE,
        ] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            fake.fail(table, Fail::LandedThenError);
            assert!(ingest_new_block(&fake, &i, &block(31)).await.is_err());
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!(
                (t.sends, t.status),
                (1, TableStatus::Pending),
                "the send was recorded before it started"
            );
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_once(&fake, EntryKind::Stream, 31);
            assert!(d.files().is_empty(), "{table}");
        }
    }

    /// A token recorded without its part (out/DEDUP-PROOF.md H1) makes the re-send a silent no-op. For a table whose
    /// rows of the block can be counted, the acknowledged re-send is confirmed and, with no rows, the rows are sent
    /// under the next token generation.
    #[tokio::test]
    async fn acknowledged_resend_without_rows_is_sent_again_under_the_next_generation() {
        for table in [
            "world_traces",
            "world_token_transfers",
            keyed_derived_after_transfers(32),
            LEDGER_TABLE,
        ] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            fake.fail(table, Fail::Unbacked);
            assert!(ingest_new_block(&fake, &i, &block(32)).await.is_err());
            assert_eq!(fake.landed(table), 0);
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_once(&fake, EntryKind::Stream, 32);
            let base = ingest_journal::dedup_token(table, 32, &block(32).hash);
            assert_eq!(
                fake.tokens_sent(table),
                vec![base.clone(), base.clone(), format!("{base}:g1")],
                "{table}"
            );
            assert!(d.files().is_empty(), "{table}");
        }
    }

    /// H1 on an unkeyed table cannot be distinguished from lost commit evidence. It parks before any automatic
    /// re-send; an operator-confirmed retry uses the next generation.
    #[tokio::test]
    async fn unkeyed_resend_without_commit_evidence_is_parked_and_retried_under_the_next_generation(
    ) {
        let table = "world_contract_interactions";
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail(table, Fail::Unbacked);
        assert!(ingest_new_block(&fake, &i, &block(33)).await.is_err());
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(d.entry(&i).parked(), vec![table]);
        assert_eq!(fake.landed(table), 0);
        assert_eq!(
            fake.landed(LEDGER_TABLE),
            0,
            "no ledger row while a table is parked"
        );
        let base = ingest_journal::dedup_token(table, 33, &block(33).hash);
        drop(i);
        ingest_journal::resolve(
            &d.0.join("pending"),
            &d.0.join("writer.lock"),
            33,
            table,
            ingest_journal::Resolution::Retry,
        )
        .unwrap();
        let i = d.ingest();
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(
            fake.tokens_sent(table).last().unwrap(),
            &format!("{base}:g1")
        );
        assert_once(&fake, EntryKind::Stream, 33);
        assert!(d.files().is_empty());
    }

    /// Lost terminal evidence cannot be replaced by the operator's presence assertion. The rows remain once,
    /// ownership survives restarts, and no ledger row claims that the unresolved send can no longer arrive.
    #[tokio::test]
    async fn unkeyed_send_that_landed_without_any_record_counts_once_and_is_parked() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "api_token_balances";
        fake.fail(table, Fail::LandedNoRecord);
        assert!(ingest_new_block(&fake, &i, &block(34)).await.is_err());
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(fake.landed(table), 1);
        assert_eq!(d.entry(&i).parked(), vec![table]);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        drop(i);
        for how in [
            ingest_journal::Resolution::Done,
            ingest_journal::Resolution::Retry,
        ] {
            assert!(ingest_journal::resolve(
                &d.0.join("pending"),
                &d.0.join("writer.lock"),
                34,
                table,
                how,
            )
            .is_err());
        }
        let i = d.ingest();
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(fake.landed(table), 1);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert_eq!(d.entry(&i).parked(), vec![table]);
        assert_eq!(fake.landed(LEDGER_TABLE), 0);
        assert_eq!(d.files(), vec!["34.block", "34.json"]);
    }

    /// A token-linked commit under a rewritten ID proves rows landed, not that the original request is terminal.
    /// Both keyed and unkeyed tables retain ownership without replay or an unfenced operator completion.
    #[tokio::test]
    async fn foreign_query_id_commit_does_not_fence_the_original_send() {
        for table in [
            "world_mev_hourly_stats",
            "world_token_transfers",
            "world_traces",
            LEDGER_TABLE,
        ] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            fake.fail(table, Fail::LandedForeignRecord);
            assert!(ingest_new_block(&fake, &i, &block(35)).await.is_err());
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_eq!(fake.landed(table), 1);
            assert_eq!(fake.tokens_sent(table).len(), 1);
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!(t.status, TableStatus::Parked);
            assert!(!t.sends_fenced());
            if table != LEDGER_TABLE {
                assert_eq!(fake.landed(LEDGER_TABLE), 0);
            }
            drop(i);
            assert!(ingest_journal::resolve(
                &d.0.join("pending"),
                &d.0.join("writer.lock"),
                35,
                table,
                ingest_journal::Resolution::Done,
            )
            .is_err());
            let i = d.ingest();
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_eq!(fake.tokens_sent(table).len(), 1);
            assert_eq!(fake.landed(table), 1);
            assert_eq!(d.files(), vec!["35.block", "35.json"]);
        }
    }

    /// A count narrowed to this entry's ingested_at may miss rows with another ingested_at. A committed part takes
    /// precedence over that empty count: no copy is sent under the next generation.
    #[tokio::test]
    async fn own_rows_count_without_rows_is_not_taken_for_a_poisoned_token() {
        let table = "api_internal_transfers";
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail(table, Fail::LandedThenError);
        fake.uncounted.borrow_mut().insert(table.to_string());
        assert!(ingest_new_block(&fake, &i, &block(36)).await.is_err());
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_once(&fake, EntryKind::Stream, 36);
        assert!(d.files().is_empty());
    }

    /// The same table under a poisoned token (DEDUP-PROOF H1): no rows counted and no committed part, so it is parked,
    /// never sent under the next generation automatically.
    #[tokio::test]
    async fn own_rows_count_and_no_commit_record_is_parked() {
        let table = "api_internal_transfers";
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail(table, Fail::Unbacked);
        assert!(ingest_new_block(&fake, &i, &block(37)).await.is_err());
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(d.entry(&i).parked(), vec![table]);
        assert_eq!(fake.landed(table), 0);
    }

    /// A send older than the deduplication windows (a schema rollback, then a new install, forgets it) is never sent
    /// again blind: a table keyed by block is counted, a table without block_number needs a committed record or is
    /// parked.
    #[tokio::test]
    async fn sends_before_the_windows_were_set_are_counted_or_parked_never_resent_blind() {
        let d = Dir::new();
        let fake = Fake::default();
        {
            let i = d.ingest();
            fake.fail("world_token_transfers", Fail::LandedThenError);
            assert!(ingest_new_block(&fake, &i, &block(41)).await.is_err());
            let mut e = d.entry(&i);
            let t = e
                .tables
                .iter_mut()
                .find(|t| t.table == "api_token_balances")
                .unwrap();
            assert_eq!((t.sends, t.status), (0, TableStatus::Pending));
            t.sends = 1; // its send started, and never landed, before the rollback
            t.first_send_ms = Some(ingest_journal::wall_ms() - 1);
            i.journal.save(&mut e).unwrap();
        }
        let since = ingest_journal::wall_ms() + 1;
        fs::write(
            d.0.join("cutover.json"),
            format!(r#"{{"version":1,"floor":10,"complete_tip":9,"created_at_ms":{since}}}"#),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(3));
        let i = d.ingest();
        assert_eq!(i.windows_since_ms, since);
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(
            fake.tokens_sent("world_token_transfers").len(),
            1,
            "counted (rows present), not sent again"
        );
        assert_eq!(fake.landed("world_token_transfers"), 1);
        assert!(fake
            .counts_asked
            .borrow()
            .contains(&"world_token_transfers".to_string()));
        assert_eq!(
            d.entry(&i).parked(),
            vec!["api_token_balances"],
            "no block key and no record: parked"
        );
        assert_eq!(fake.landed("api_token_balances"), 0, "never sent blind");
    }

    #[tokio::test]
    async fn rejected_and_refused_sends_are_retried_until_written() {
        for table in [
            "world_traces",
            "world_token_transfers",
            "api_internal_transfers",
            "world_contract_interactions",
            LEDGER_TABLE,
        ] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            fake.fail(table, Fail::Rejected);
            fake.fail(table, Fail::Refused);
            assert!(ingest_new_block(&fake, &i, &block(20)).await.is_err());
            if table == "world_traces" {
                assert_eq!(
                    fake.landed("world_blocks"),
                    0,
                    "the block row waits for the traces"
                );
            }
            assert!(resume_stream_entries(&fake, &i).await.is_err());
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!((t.sends, t.generation), (2, 0));
            assert!(
                t.all_rejected(),
                "{table}: rejection and typed refusal are both proven safe"
            );
            fake.sends.borrow_mut().clear();
            assert_eq!(
                resume_stream_entries(&fake, &i).await.unwrap(),
                20,
                "the newest block the journal held"
            );
            assert_eq!(
                fake.sends.borrow()[0],
                table,
                "earlier tables are not sent again"
            );
            let base = ingest_journal::dedup_token(table, 20, &block(20).hash);
            assert_eq!(
                fake.tokens_sent(table),
                vec![base.clone(), base.clone(), base]
            );
            assert_once(&fake, EntryKind::Stream, 20);
            assert!(d.files().is_empty(), "{table}");
        }
    }

    /// ExceptionWhileProcessing is terminal, but is not ExceptionBeforeStart: a prefix can be committed without
    /// surviving NewPart evidence, so an unkeyed table must not be replayed as though the request was rejected.
    #[tokio::test]
    async fn unkeyed_processing_exception_with_partial_rows_is_parked_without_replay() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_contract_interactions";
        fake.fail(table, Fail::ProcessingException);
        assert!(ingest_new_block(&fake, &i, &block(91)).await.is_err());
        resume_stream_entries(&fake, &i).await.unwrap();
        let e = d.entry(&i);
        let t = e.tables.iter().find(|t| t.table == table).unwrap();
        assert_eq!(t.status, TableStatus::Parked);
        assert!(t.sends_fenced());
        assert!(!t.all_rejected());
        assert!(t.rejected_query_ids.is_empty());
        assert_eq!(
            fake.landed(table),
            1,
            "the committed prefix is not repeated"
        );
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert_eq!(fake.landed(LEDGER_TABLE), 0);
        drop(i);
        let i = d.ingest();
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(fake.landed(table), 1);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert_eq!(d.entry(&i).parked(), vec![table]);
        assert_eq!(d.files(), vec!["91.block", "91.json"]);
    }

    /// Older engines could have several in-flight sends, and v3 could truncate their recorded IDs. A terminal
    /// newest send (even a committed one with a positive count) cannot fence older or forgotten requests.
    #[tokio::test]
    async fn older_sends_and_legacy_truncated_ids_cannot_be_replayed_by_fencing_only_the_newest() {
        for truncated in [false, true] {
            for newest in [Fail::Rejected, Fail::LandedThenError] {
                let d = Dir::new();
                let i = d.ingest();
                let fake = Fake::default();
                let b = block(90);
                let table = "world_token_transfers";
                fake.fail(table, Fail::Hidden);
                assert!(ingest_new_block(&fake, &i, &b).await.is_err());
                let mut e = d.entry(&i);
                let ti = e.tables.iter().position(|t| t.table == table).unwrap();
                let at = ingest_journal::wall_ms();
                let second = ingest_journal::query_id(90, table, at);
                let last = ingest_journal::query_id(90, table, at);
                e.tables[ti].record_send(&second, at).unwrap();
                e.tables[ti].record_send(&last, at).unwrap();
                i.journal.save(&mut e).unwrap();
                let batches = build_batches(&b, e.now_ms);
                let token = ingest_journal::dedup_token(table, 90, &b.hash);
                fake.fail(table, Fail::Hidden);
                assert!(fake
                    .insert(&batches, &e, table, &token, &second)
                    .await
                    .is_err());
                fake.fail(table, newest);
                assert!(fake
                    .insert(&batches, &e, table, &token, &last)
                    .await
                    .is_err());
                if truncated {
                    // A legacy history that forgot every id but the newest also has no client
                    // proof for the forgotten ids. Keeping those proofs would be a corrupt map,
                    // which save rejects; the forgotten sends stay unrecoverable because `sends`
                    // still exceeds `query_ids`.
                    e.tables[ti].query_ids = vec![last.clone()];
                    e.tables[ti].clients.retain(|id, _| id == &last);
                    i.journal.save(&mut e).unwrap();
                }
                resume_stream_entries(&fake, &i).await.unwrap();
                let e = d.entry(&i);
                let t = &e.tables[ti];
                assert_eq!(
                    (t.sends, t.generation, t.status),
                    (3, 0, TableStatus::Parked)
                );
                assert_eq!(t.query_ids.len(), if truncated { 1 } else { 3 });
                assert_eq!(t.terminal_query_ids, vec![last]);
                assert!(!t.sends_fenced());
                assert!(!t.all_rejected());
                assert_eq!(
                    fake.tokens_sent(table).len(),
                    3,
                    "no fourth send or new generation"
                );
                assert_eq!(
                    fake.landed(table),
                    u32::from(matches!(newest, Fail::LandedThenError))
                );
                assert_eq!(fake.landed(LEDGER_TABLE), 0);
                assert_eq!(d.files(), vec!["90.block", "90.json"]);
                drop(i);
                for how in [
                    ingest_journal::Resolution::Done,
                    ingest_journal::Resolution::Retry,
                ] {
                    assert!(ingest_journal::resolve(
                        &d.0.join("pending"),
                        &d.0.join("writer.lock"),
                        90,
                        table,
                        how,
                    )
                    .is_err());
                }
                let i = d.ingest();
                fake.deliver_hidden();
                resume_stream_entries(&fake, &i).await.unwrap();
                assert_eq!(fake.landed(table), 1);
                assert_eq!(fake.tokens_sent(table).len(), 3);
                let e = d.entry(&i);
                assert_eq!(e.tables[ti].status, TableStatus::Parked);
                if truncated {
                    assert!(
                        !e.tables[ti].sends_fenced(),
                        "forgotten IDs cannot be reconstructed by latest-only evidence"
                    );
                    assert_eq!(fake.landed(LEDGER_TABLE), 0);
                    assert_eq!(d.files(), vec!["90.block", "90.json"]);
                } else {
                    assert!(e.tables[ti].sends_fenced());
                    drop(i);
                    ingest_journal::resolve(
                        &d.0.join("pending"),
                        &d.0.join("writer.lock"),
                        90,
                        table,
                        ingest_journal::Resolution::Done,
                    )
                    .unwrap();
                    let i = d.ingest();
                    resume_stream_entries(&fake, &i).await.unwrap();
                    assert_once(&fake, EntryKind::Stream, 90);
                    assert!(d.files().is_empty());
                }
            }
        }
    }

    /// Review finding 7: rows another writer stored (raw heal, trace-gap healer) before this entry sends a core table
    /// are counted under the block lock and not written again, on the fresh path too.
    #[tokio::test]
    async fn core_rows_another_writer_stored_are_not_written_again_fresh_or_resumed() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.foreign.borrow_mut().insert("world_traces".into(), 7); // the trace-gap healer was first
        assert!(ingest_new_block(&fake, &i, &block(21)).await.unwrap());
        assert_eq!(
            fake.landed("world_traces"),
            0,
            "fresh path: counted, not sent"
        );
        assert_eq!(fake.landed("world_logs"), 1);

        let fake = Fake::default();
        fake.fail("world_logs", Fail::Rejected);
        assert!(ingest_new_block(&fake, &i, &block(22)).await.is_err());
        fake.foreign.borrow_mut().insert("world_traces".into(), 7); // healed while the stream was stopped
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(fake.landed("world_traces"), 0);
        assert_eq!(fake.landed("world_logs"), 1);
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn ledger_row_present_means_every_table_is_written() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.foreign.borrow_mut().insert(LEDGER_TABLE.into(), 1);
        assert!(ingest_new_block(&fake, &i, &block(23)).await.unwrap());
        assert!(fake.sends.borrow().is_empty(), "{:?}", fake.sends.borrow());
        assert!(d.files().is_empty());
    }

    /// Historical Done bits and another writer's ledger row cannot release an owner whose old send may still
    /// arrive. Cover both the all-Done restart shortcut and the normal pending-table presence shortcut.
    #[tokio::test]
    async fn legacy_done_bits_and_existing_ledger_cannot_discard_an_unfenced_owner() {
        for all_done in [false, true] {
            for ledger_present in [false, true] {
                let d = Dir::new();
                let i = d.ingest();
                let fake = Fake::default();
                let table = "world_contract_interactions";
                fake.fail(table, Fail::Hidden);
                assert!(ingest_new_block(&fake, &i, &block(92)).await.is_err());
                let mut e = d.entry(&i);
                for t in &mut e.tables {
                    if all_done || t.table == table {
                        t.status = TableStatus::Done;
                    }
                }
                i.journal.save(&mut e).unwrap();
                if ledger_present {
                    fake.foreign.borrow_mut().insert(LEDGER_TABLE.into(), 1);
                }
                resume_stream_entries(&fake, &i).await.unwrap();
                let e = d.entry(&i);
                let t = e.tables.iter().find(|t| t.table == table).unwrap();
                assert_eq!(t.status, TableStatus::Parked);
                assert!(!t.sends_fenced());
                assert_eq!(fake.tokens_sent(table).len(), 1);
                assert_eq!(fake.landed(table), 0);
                assert_eq!(fake.landed(LEDGER_TABLE), 0, "no new completion marker");
                assert_eq!(d.files(), vec!["92.block", "92.json"]);
                drop(i);
                assert!(ingest_journal::resolve(
                    &d.0.join("pending"),
                    &d.0.join("writer.lock"),
                    92,
                    table,
                    ingest_journal::Resolution::Done,
                )
                .is_err());
                let i = d.ingest();
                resume_stream_entries(&fake, &i).await.unwrap();
                assert_eq!(d.entry(&i).parked(), vec![table]);
                assert_eq!(fake.tokens_sent(table).len(), 1);
                assert_eq!(d.files(), vec!["92.block", "92.json"]);
            }
        }
    }

    /// A derived table after world_token_transfers in write order whose count covers every writer's rows.
    fn keyed_derived_after_transfers(n: u64) -> &'static str {
        tables_of(EntryKind::Stream, n)
            .into_iter()
            .skip_while(|t| *t != "world_token_transfers")
            .skip(1)
            .find(|t| {
                table_class(t) == TableClass::Derived && ingest_journal::count_proves_absence(t)
            })
            .expect("the fixture block has a keyed derived table after world_token_transfers")
    }

    #[tokio::test]
    async fn sends_beyond_the_horizon_are_counted_or_parked_never_resent_blind() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let b = block(40);
        let keyed = keyed_derived_after_transfers(40);
        fake.fail("world_token_transfers", Fail::LandedThenError);
        assert!(ingest_new_block(&fake, &i, &b).await.is_err());
        let mut e = d.entry(&i);
        let old = ingest_journal::wall_ms() - RESEND_HORIZON_MS - 1_000;
        let mut delayed = Vec::new();
        for t in &mut e.tables {
            if t.table == "world_token_transfers" {
                t.first_send_ms = Some(old);
            } else if ["api_token_balances", "api_internal_transfers", keyed]
                .contains(&t.table.as_str())
            {
                assert_eq!(
                    (t.sends, t.status),
                    (0, TableStatus::Pending),
                    "{}",
                    t.table
                );
                let query_id = ingest_journal::query_id(40, &t.table, old);
                t.record_send(&query_id, old).unwrap();
                delayed.push((t.table.clone(), query_id));
            }
        }
        i.journal.save(&mut e).unwrap();
        let batches = build_batches(&b, e.now_ms);
        for (table, query_id) in delayed {
            fake.fail(&table, Fail::Hidden);
            let token = ingest_journal::dedup_token(&table, 40, &b.hash);
            assert!(fake
                .insert(&batches, &e, &table, &token, &query_id)
                .await
                .is_err());
        }
        resume_stream_entries(&fake, &i).await.unwrap();
        let e = d.entry(&i);
        let mut parked = e.parked();
        parked.sort();
        let mut expected = vec!["api_internal_transfers", "api_token_balances", keyed];
        expected.sort();
        assert_eq!(
            parked, expected,
            "an old keyed zero count cannot fence a delayed send either"
        );
        assert_eq!(fake.landed("api_token_balances"), 0, "never sent again");
        assert_eq!(
            fake.landed("api_internal_transfers"),
            0,
            "its count sees only this entry's rows: never sent again blind"
        );
        assert_eq!(
            fake.landed("world_token_transfers"),
            1,
            "terminal and present, not sent again"
        );
        assert_eq!(fake.tokens_sent("world_token_transfers").len(), 1);
        assert_eq!(
            fake.landed(keyed),
            0,
            "no replay before the exact request is terminal"
        );
        assert!(!fake.counts_asked.borrow().contains(&keyed.to_string()));
        assert_eq!(
            fake.landed(LEDGER_TABLE),
            0,
            "no ledger row while a table is parked"
        );
        assert_eq!(
            d.files(),
            vec!["40.block", "40.json"],
            "kept for the operator"
        );
        fake.deliver_hidden();
        resume_stream_entries(&fake, &i).await.unwrap();
        assert!(d.entry(&i).tables.iter().all(|t| t.sends_fenced()));
        drop(i);
        ingest_journal::resolve(
            &d.0.join("pending"),
            &d.0.join("writer.lock"),
            40,
            "all",
            ingest_journal::Resolution::Done,
        )
        .unwrap();
        let i = d.ingest();
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_once(&fake, EntryKind::Stream, 40);
        assert_eq!(fake.landed(LEDGER_TABLE), 1);
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn saved_but_uncommitted_block_is_written_on_resume() {
        let d = Dir::new();
        let fake = Fake::default();
        {
            let i = d.ingest();
            fake.fail("world_transactions", Fail::Refused);
            assert!(ingest_new_block(&fake, &i, &block(50)).await.is_err());
        }
        fs::remove_file(d.0.join("pending/50.json")).unwrap(); // as if the stop came before the entry commit
        let i = d.ingest();
        assert_eq!(resume_stream_entries(&fake, &i).await.unwrap(), 50);
        assert_once(&fake, EntryKind::Stream, 50);
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn first_core_send_confirms_a_healer_poisoned_token() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let b = block(60);
        let token = ingest_journal::dedup_token("world_traces", b.number, &b.hash);
        fake.tokens
            .borrow_mut()
            .insert(("world_traces".into(), token.clone()));
        assert!(ingest_new_block(&fake, &i, &b).await.unwrap());
        assert_eq!(
            fake.tokens_sent("world_traces"),
            vec![token.clone(), ingest_journal::generation_token(&token, 1)]
        );
        assert_once(&fake, EntryKind::Stream, b.number);
    }

    #[tokio::test]
    async fn lost_payload_waits_for_raw_heal_without_forgetting_done_tables() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail("world_logs", Fail::Rejected);
        assert!(ingest_new_block(&fake, &i, &block(60)).await.is_err());
        let before = d.entry(&i).tables;
        fs::remove_file(d.0.join("pending/60.block")).unwrap();
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(d.entry(&i).tables, before);
        assert_eq!(d.files(), vec!["60.json", "gap-60-60.json"]);
        process_gap_claims(&fake, &i.journal).await.unwrap();
        assert_eq!(fake.queue.borrow().len(), 1);
        assert_eq!(
            d.files(),
            vec!["60.json"],
            "the claim never replaces the entry"
        );
        *fake.rebuilt.borrow_mut() = Some(block(60));
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_once(&fake, EntryKind::Stream, 60);
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn lost_payload_preserves_a_parked_poisoned_token() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_contract_interactions";
        fake.fail(table, Fail::Unbacked);
        assert!(ingest_new_block(&fake, &i, &block(61)).await.is_err());
        resume_stream_entries(&fake, &i).await.unwrap();
        let before = d.entry(&i).tables;
        assert!(before
            .iter()
            .any(|t| t.table == table && t.status == TableStatus::Parked));
        fs::remove_file(d.0.join("pending/61.block")).unwrap();
        *fake.rebuilt.borrow_mut() = Some(block(61));
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(d.entry(&i).tables, before);
        assert_eq!(fake.landed(table), 0);
        assert_eq!(
            fake.landed(LEDGER_TABLE),
            0,
            "no ledger for an unproven send"
        );
        assert!(i.journal.entry_payload(&d.entry(&i)).unwrap().is_some());
    }

    #[tokio::test]
    async fn lost_payload_keeps_done_tables_after_their_tokens_expire() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail("api_gas_by_function", Fail::Rejected);
        assert!(ingest_new_block(&fake, &i, &block(62)).await.is_err());
        let mut e = d.entry(&i);
        for t in e
            .tables
            .iter_mut()
            .filter(|t| t.status == TableStatus::Done)
        {
            t.first_send_ms = Some(ingest_journal::wall_ms() - RESEND_HORIZON_MS - 1);
        }
        i.journal.save(&mut e).unwrap();
        fake.evict("world_contract_interactions");
        fs::remove_file(d.0.join("pending/62.block")).unwrap();
        *fake.rebuilt.borrow_mut() = Some(block(62));
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(fake.landed("world_contract_interactions"), 1);
        assert_once(&fake, EntryKind::Stream, 62);
        assert_eq!(
            fake.tokens_sent("world_contract_interactions").len(),
            1,
            "Done never replays"
        );
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn lost_payload_with_different_table_rows_is_parked() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail("world_logs", Fail::Rejected);
        assert!(ingest_new_block(&fake, &i, &block(63)).await.is_err());
        fs::remove_file(d.0.join("pending/63.block")).unwrap();
        let mut rebuilt = block(63);
        rebuilt.logs.clear();
        *fake.rebuilt.borrow_mut() = Some(rebuilt);
        resume_stream_entries(&fake, &i).await.unwrap();
        let e = d.entry(&i);
        assert_eq!(e.tables[0].status, TableStatus::Done);
        assert!(e.parked().contains(&"world_logs"));
        assert_eq!(fake.landed(LEDGER_TABLE), 0);
    }

    #[tokio::test]
    async fn unknown_unkeyed_send_is_parked_after_early_token_eviction() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_contract_interactions";
        fake.fail(table, Fail::LandedNoRecord);
        assert!(ingest_new_block(&fake, &i, &block(64)).await.is_err());
        fake.evict(table);
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_eq!(fake.landed(table), 1);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert!(d.entry(&i).parked().contains(&table));
        assert_eq!(fake.landed(LEDGER_TABLE), 0);
    }

    #[tokio::test]
    async fn committed_unkeyed_send_is_not_repeated_after_token_eviction() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_contract_interactions";
        fake.fail(table, Fail::LandedThenError);
        assert!(ingest_new_block(&fake, &i, &block(65)).await.is_err());
        fake.evict(table);
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_once(&fake, EntryKind::Stream, 65);
        assert_eq!(
            fake.tokens_sent(table).len(),
            1,
            "positive evidence takes precedence over retention"
        );
    }

    /// Review finding 5: the journal can't save the received block (disk full, permissions). v1 returned the error and
    /// the block was gone. v2 keeps it and saves it again until the journal takes it, then writes it.
    #[tokio::test]
    async fn received_block_is_kept_and_journaled_again_after_a_write_failure() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let pending = d.0.join("pending");
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o555)).unwrap();
        let p2 = pending.clone();
        let restore = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1_500)).await;
            fs::set_permissions(&p2, fs::Permissions::from_mode(0o755)).unwrap();
        });
        assert!(
            ingest_new_block(&fake, &i, &block(70)).await.unwrap(),
            "finished once the journal took it"
        );
        restore.await.unwrap();
        assert_once(&fake, EntryKind::Stream, 70);
        assert!(d.files().is_empty());
    }

    /// Review finding 3: the gap is durable before the higher block is taken, and its enqueue is retried from the
    /// journal until ClickHouse acknowledges it, even though the higher block committed long before.
    #[tokio::test]
    async fn gap_is_claimed_before_the_higher_block_and_enqueued_until_acknowledged() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let mut pos = StreamPos {
            expected: 100,
            first: false,
            cutover: None,
        };
        fake.fail("world_transactions", Fail::Rejected); // the higher block itself fails first
        assert!(handle_block(&fake, &i, &mut pos, &block(102))
            .await
            .is_err());
        assert_eq!(
            pos.expected, 100,
            "the cursor does not move past an unfinished block"
        );
        assert_eq!(d.files(), vec!["102.block", "102.json", "gap-100-101.json"]);
        resume_stream_entries(&fake, &i).await.unwrap(); // reconnect
        pos.expected = 103;
        *fake.enqueue_failures.borrow_mut() = 2; // world_sequence_gaps refuses the first two enqueues
        process_gap_claims(&fake, &i.journal).await.unwrap();
        process_gap_claims(&fake, &i.journal).await.unwrap();
        assert_eq!(
            d.files(),
            vec!["gap-100-101.json"],
            "still claimed after two failed enqueues"
        );
        assert!(
            handle_block(&fake, &i, &mut pos, &block(103))
                .await
                .unwrap(),
            "the stream goes on meanwhile"
        );
        process_gap_claims(&fake, &i.journal).await.unwrap();
        assert_eq!(
            fake.queue
                .borrow()
                .iter()
                .map(|q| (q.0, q.1))
                .collect::<Vec<_>>(),
            vec![(100, 101)]
        );
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn real_transport_distinguishes_connect_refusal_from_post_submission_close() {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let batches = build_batches(&block(93), 1);
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = Client::default().with_url(format!("http://{address}"));
        let refused = tokio::time::timeout(
            Duration::from_secs(5),
            insert_rows(
                &client,
                "world_blocks",
                &batches.blocks,
                "refused-token",
                "refused-query",
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(
            refused.not_submitted,
            "the connection never opened: {refused}"
        );

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4_096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            drop(stream);
        });
        let client = Client::default().with_url(format!("http://{address}"));
        let closed = tokio::time::timeout(
            Duration::from_secs(5),
            insert_rows(
                &client,
                "world_blocks",
                &batches.blocks,
                "closed-token",
                "closed-query",
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        peer.await.unwrap();
        assert!(
            !closed.not_submitted,
            "accepted request has an unknown outcome: {closed}"
        );
    }

    /// A write that fails before `Insert::end` can already have committed a prefix. The durable send stays
    /// unfenced and not rejected. The next attempt does not count it, replay it, advance its generation, mark it
    /// Done, or remove the owner.
    #[tokio::test]
    async fn pre_end_write_failure_keeps_history_and_does_not_replay() {
        let table = "world_transactions";
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        fake.fail(table, Fail::PrefixWrite);
        assert!(ingest_new_block(&fake, &i, &block(64)).await.is_err());
        let failed = d.entry(&i);
        let state = failed.tables.iter().find(|t| t.table == table).unwrap();
        let query_id = state.query_ids[0].clone();
        assert_eq!(
            (state.sends, state.generation, state.status),
            (1, 0, TableStatus::Pending)
        );
        assert!(state.terminal_query_ids.is_empty());
        assert!(state.rejected_query_ids.is_empty());
        assert!(!state.all_rejected());
        assert!(!state.sends_fenced());
        assert!(state.clients[&query_id].ended_ms.is_some());
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert_eq!(fake.landed(table), 0);

        resume_stream_entries(&fake, &i).await.unwrap();
        let e = d.entry(&i);
        let t = e.tables.iter().find(|t| t.table == table).unwrap();
        assert_eq!(
            (t.status, t.generation, t.sends),
            (TableStatus::Parked, 0, 1)
        );
        assert_eq!(t.query_ids, vec![query_id]);
        assert!(t.terminal_query_ids.is_empty());
        assert!(t.rejected_query_ids.is_empty());
        assert!(!t.sends_fenced());
        assert!(!t.all_rejected());
        assert_eq!(fake.tokens_sent(table).len(), 1, "no replay");
        assert_eq!(fake.landed(table), 0);
        assert!(
            !fake
                .counts_asked
                .borrow()
                .iter()
                .any(|asked| asked == table),
            "no count before the send is fenced: {:?}",
            fake.counts_asked.borrow()
        );
        assert_eq!(fake.landed(LEDGER_TABLE), 0, "owner not finished");
        assert_eq!(d.files(), vec!["64.block", "64.json"]);
    }

    #[tokio::test]
    async fn high_heal_owner_does_not_skip_lower_gaps_or_accept_a_second_stream_owner() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let high = block(110);
        let mut pos = StreamPos {
            expected: 101,
            first: false,
            cutover: None,
        };
        i.stream_expected.store(101, Ordering::SeqCst);
        {
            let _owner = i.locks.lock(high.number).await;
            i.journal
                .create_heal(high.number, &high.hash, high.timestamp, 1)
                .unwrap();
        }
        let before = fs::read(d.0.join("pending/110.json")).unwrap();
        assert_eq!(
            resume_stream_entries(&fake, &i).await.unwrap(),
            0,
            "a high Heal owner says nothing about lower unclaimed heights"
        );
        assert_eq!(i.stream_expected.load(Ordering::SeqCst), 101);
        assert!(fake.sends.borrow().is_empty());
        assert!(!handle_block(&fake, &i, &mut pos, &high).await.unwrap());
        assert_eq!(pos.expected, 111);
        assert_eq!(i.stream_expected.load(Ordering::SeqCst), 111);
        assert!(
            fake.sends.borrow().is_empty(),
            "the Heal height was not streamed again"
        );
        let claims = i.journal.claims().unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!((claims[0].lo, claims[0].hi), (101, 109));
        assert_eq!(fs::read(d.0.join("pending/110.json")).unwrap(), before);
        assert!(handle_block(&fake, &i, &mut pos, &block(111))
            .await
            .unwrap());
        assert_eq!(pos.expected, 112);
        assert_once(&fake, EntryKind::Stream, 111);
        assert_eq!(fs::read(d.0.join("pending/110.json")).unwrap(), before);
        assert_eq!(d.files(), vec!["110.json", "gap-101-109.json"]);
    }

    /// Review finding 4: a long outage gap (12,600 heights) is kept whole and enqueued in chunks of at most 10,000.
    #[tokio::test]
    async fn long_gap_is_enqueued_whole_in_bounded_chunks() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let mut pos = StreamPos {
            expected: 36_000_101,
            first: true,
            cutover: None,
        };
        assert!(handle_block(&fake, &i, &mut pos, &block(36_012_701))
            .await
            .unwrap());
        process_gap_claims(&fake, &i.journal).await.unwrap();
        let q: Vec<(u64, u64)> = fake.queue.borrow().iter().map(|q| (q.0, q.1)).collect();
        assert_eq!(q, vec![(36_000_101, 36_010_100), (36_010_101, 36_012_700)]);
        assert!(fake
            .queue
            .borrow()
            .iter()
            .all(|r| r.3 == "streamer-reconnect"));
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn block_below_the_cursor_is_not_written_again() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let mut pos = StreamPos {
            expected: 200,
            first: false,
            cutover: None,
        };
        assert!(!handle_block(&fake, &i, &mut pos, &block(199))
            .await
            .unwrap());
        assert!(fake.sends.borrow().is_empty());
        assert_eq!(pos.expected, 200);
    }

    #[tokio::test]
    async fn first_stream_after_the_install_claims_the_cutover_gap_once() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let c = Cutover::load(&i.cutover_path).unwrap();
        let mut pos = start_position(13, c); // the old engine's transactions moved the tip past the floor (10)
        assert_eq!(pos.expected, 10);
        assert!(handle_block(&fake, &i, &mut pos, &block(15)).await.unwrap());
        assert_eq!(d.files(), vec!["gap-10-14.json"]);
        let c = Cutover::load(&i.cutover_path).unwrap();
        assert_eq!(c.claimed_gap, Some([10, 14]));
        let again = start_position(16, c);
        assert_eq!(
            (again.expected, again.cutover.is_none()),
            (16, true),
            "claimed once"
        );
    }

    /// Derived ownership has the same fence as the stream: a hidden keyed request parks without a replay, and the
    /// ledger waits for exact terminal evidence plus the operator's confirmation of the late rows.
    #[tokio::test]
    async fn derive_entry_writes_derived_tables_and_the_ledger_once() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_token_transfers";
        fake.fail(table, Fail::Hidden);
        assert!(derive_block(&fake, &i, &block(80), 1_760_000_000_000)
            .await
            .is_err());
        let loaded = i.journal.load(EntryKind::Derive).unwrap();
        assert_eq!(loaded.len(), 1);
        for item in loaded {
            resume_loaded(&fake, &i, item).await.unwrap();
        }
        let entry = match i.journal.load(EntryKind::Derive).unwrap().pop().unwrap() {
            Loaded::Entry(entry) => entry,
            other => panic!("expected derive entry, got {other:?}"),
        };
        assert_eq!(entry.parked(), vec![table]);
        assert_eq!(
            fake.tokens_sent(table).len(),
            1,
            "no blind same-token retry"
        );
        assert_eq!(fake.landed(LEDGER_TABLE), 0);
        assert_eq!(d.files(), vec!["80.block", "80.json"]);
        fake.deliver_hidden();
        for item in i.journal.load(EntryKind::Derive).unwrap() {
            resume_loaded(&fake, &i, item).await.unwrap();
        }
        drop(i);
        ingest_journal::resolve(
            &d.0.join("pending"),
            &d.0.join("writer.lock"),
            80,
            table,
            ingest_journal::Resolution::Done,
        )
        .unwrap();
        let i = d.ingest();
        for item in i.journal.load(EntryKind::Derive).unwrap() {
            resume_loaded(&fake, &i, item).await.unwrap();
        }
        assert_once(&fake, EntryKind::Derive, 80);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert_eq!(
            fake.landed("world_transactions"),
            0,
            "a derive entry writes no core table"
        );
        assert!(d.files().is_empty());
    }
    #[test]
    fn same_boot_absence_requires_raw_client_end_and_strictly_later_restart_with_uptime_rounding() {
        let mut entry = Entry::new(EntryKind::Stream, 1, "0x1", 1, 1000, &[("world_traces", 1)]);
        let t = &mut entry.tables[0];
        t.record_send("q1", 1000).unwrap();
        let boot_id = ingest_journal::current_boot_id().unwrap();
        let server = ServerFence::from_uptime(boot_id, 5001, 1).unwrap();
        assert!(
            absence_candidates(t, server).query_ids.is_empty(),
            "a client without an end cannot be fenced by restart"
        );
        t.record_client_end("q1", 2000).unwrap();
        t.clients.get_mut("q1").unwrap().ended_raw_ms = Some(3000);
        for before_raw_ms in [4000, 4999, 5000] {
            let rounded = ServerFence::from_uptime(boot_id, before_raw_ms, 1).unwrap();
            assert!(
                absence_candidates(t, rounded).query_ids.is_empty(),
                "startup's rounded lower bound must be strictly after raw end, not merely send start"
            );
        }
        assert_eq!(absence_candidates(t, server).query_ids, ["q1"]);
        t.clients.get_mut("q1").unwrap().ended_raw_ms = None;
        assert!(
            absence_candidates(t, server).query_ids.is_empty(),
            "a legacy wall-only end cannot be translated into raw proof"
        );
        t.clients.clear();
        assert!(
            absence_candidates(t, server).query_ids.is_empty(),
            "legacy writer_pid alone is not per-send lifetime proof"
        );
    }

    #[test]
    fn reboot_fence_ignores_old_raw_epoch_but_requires_valid_local_server_uptime() {
        let mut entry = Entry::new(EntryKind::Stream, 1, "0x1", 1, 10, &[("world_traces", 1)]);
        let t = &mut entry.tables[0];
        t.record_send("q1", 10).unwrap();
        let boot_id = ingest_journal::current_boot_id().unwrap();
        let server = ServerFence::from_uptime(boot_id, 1000, 0).unwrap();
        t.clients.get_mut("q1").unwrap().boot_id = "previous-host-boot".to_string();
        assert!(
            absence_candidates(t, server).query_ids.is_empty(),
            "end must first be observed"
        );
        t.clients.get_mut("q1").unwrap().ended_ms = Some(u64::MAX);
        for old_raw_ms in [None, Some(1), Some(u64::MAX)] {
            t.clients.get_mut("q1").unwrap().ended_raw_ms = old_raw_ms;
            assert_eq!(
                absence_candidates(t, server).query_ids,
                ["q1"],
                "a host reboot destroys old connections without comparing raw epochs"
            );
        }
        let before = t.clone();
        for (before_raw_ms, uptime_s) in [(0, 0), (999, 0), (1000, 1), (u64::MAX, u64::MAX)] {
            assert!(ServerFence::from_uptime(boot_id, before_raw_ms, uptime_s).is_err());
            assert_eq!(*t, before, "invalid uptime cannot manufacture reboot proof");
        }
        assert!(
            !t.sends_fenced(),
            "candidates still require a successful flush and complete absence"
        );
        assert!(!t.all_rejected(), "a reboot never proves noncommit");
    }

    fn restart_after_client(t: &TableState) -> ServerFence {
        let end = t
            .clients
            .values()
            .filter_map(|c| c.ended_raw_ms)
            .max()
            .unwrap();
        ServerFence::from_uptime(
            ingest_journal::current_boot_id().unwrap(),
            end.checked_add(1001).unwrap(),
            0,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn wall_clock_steps_and_legacy_receive_deadlines_never_authorize_count_or_replay() {
        for wall_ms in [0, u64::MAX] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            let table = "world_traces";
            fake.fail(table, Fail::Undelivered);
            assert!(ingest_new_block(&fake, &i, &block(95)).await.is_err());
            let e = d.entry(&i);
            let index = e.tables.iter().position(|t| t.table == table).unwrap();
            let id = e.tables[index].query_ids[0].clone();
            let end_raw_ms = e.tables[index].clients[&id].ended_raw_ms.unwrap();
            let mut json = serde_json::to_value(e).unwrap();
            json["tables"][index]["clients"][&id]["started_ms"] = serde_json::json!(0);
            json["tables"][index]["clients"][&id]["ended_ms"] = serde_json::json!(wall_ms);
            json["tables"][index]["clients"][&id]["receive_timeout_ms"] =
                serde_json::json!(wall_ms);
            json["tables"][index]["fence_until_ms"] = serde_json::json!(wall_ms);
            let mut restored: Entry = serde_json::from_value(json).unwrap();
            i.journal.save(&mut restored).unwrap();
            *fake.server_fence.borrow_mut() = Some(
                ServerFence::from_uptime(
                    ingest_journal::current_boot_id().unwrap(),
                    end_raw_ms.checked_add(1000).unwrap(),
                    0,
                )
                .unwrap(),
            );
            fake.counts_asked.borrow_mut().clear();
            resume_stream_entries(&fake, &i).await.unwrap();
            let e = d.entry(&i);
            let t = &e.tables[index];
            assert_eq!(t.status, TableStatus::Parked);
            assert!(!t.sends_fenced());
            assert!(!t.all_rejected());
            assert_eq!(t.generation, 0);
            assert_eq!(fake.tokens_sent(table).len(), 1);
            assert!(!fake
                .counts_asked
                .borrow()
                .iter()
                .any(|asked| asked == table));
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
            assert_eq!(d.files(), ["95.block", "95.json"]);
        }
    }

    #[tokio::test]
    async fn server_raw_clock_failure_keeps_history_and_never_counts_or_replays() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_traces";
        fake.fail(table, Fail::Undelivered);
        assert!(ingest_new_block(&fake, &i, &block(96)).await.is_err());
        let before = d
            .entry(&i)
            .tables
            .into_iter()
            .find(|t| t.table == table)
            .unwrap();
        *fake.absence_error.borrow_mut() = Some("CLOCK_MONOTONIC_RAW unavailable".to_string());
        fake.counts_asked.borrow_mut().clear();
        assert!(resume_stream_entries(&fake, &i)
            .await
            .unwrap_err()
            .contains("CLOCK_MONOTONIC_RAW"));
        let after = d
            .entry(&i)
            .tables
            .into_iter()
            .find(|t| t.table == table)
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert!(fake.counts_asked.borrow().is_empty());
        assert_eq!(d.files(), ["96.block", "96.json"]);
    }

    #[tokio::test]
    async fn lost_log_counted_send_retries_only_after_cannot_arrive_proof() {
        let d = Dir::new();
        let i = d.ingest_waiting(300_000);
        let fake = Fake::default();
        let table = "world_traces";
        fake.fail(table, Fail::Undelivered);
        assert!(ingest_new_block(&fake, &i, &block(91)).await.is_err());
        fake.counts_asked.borrow_mut().clear();
        assert!(resume_stream_entries(&fake, &i).await.is_err());
        assert_eq!(
            fake.tokens_sent(table).len(),
            1,
            "no replay while the restart proof is missing"
        );
        assert!(!fake
            .counts_asked
            .borrow()
            .iter()
            .any(|asked| asked == table));
        let e = d.entry(&i);
        let t = e.tables.iter().find(|t| t.table == table).unwrap();
        *fake.server_fence.borrow_mut() = Some(restart_after_client(t));
        resume_stream_entries(&fake, &i).await.unwrap();
        assert_once(&fake, EntryKind::Stream, 91);
        assert_eq!(fake.tokens_sent(table).len(), 2);
        assert_eq!(
            fake.tokens_sent(table)[0],
            fake.tokens_sent(table)[1],
            "fenced global absence retries the same generation"
        );
        assert!(d.files().is_empty());
    }

    #[tokio::test]
    async fn lost_log_unkeyed_send_is_fenced_not_replayed_until_operator_reconciles() {
        let table = "api_token_balances";
        for (failure, how) in [
            (Fail::Undelivered, ingest_journal::Resolution::Retry),
            (Fail::LandedNoRecord, ingest_journal::Resolution::Done),
        ] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            fake.fail(table, failure);
            assert!(ingest_new_block(&fake, &i, &block(92)).await.is_err());
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            *fake.server_fence.borrow_mut() = Some(restart_after_client(t));
            resume_stream_entries(&fake, &i).await.unwrap();
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!(t.status, TableStatus::Parked);
            assert!(t.sends_fenced());
            assert!(
                !t.all_rejected(),
                "lost logs do not prove no part committed"
            );
            assert_eq!(fake.tokens_sent(table).len(), 1);
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
            drop(i);
            ingest_journal::resolve(
                &d.0.join("pending"),
                &d.0.join("writer.lock"),
                92,
                table,
                how,
            )
            .unwrap();
            let i = d.ingest();
            resume_stream_entries(&fake, &i).await.unwrap();
            assert_once(&fake, EntryKind::Stream, 92);
            assert!(d.files().is_empty());
        }
    }

    #[tokio::test]
    async fn query_start_or_live_process_blocks_absence_even_after_restart_bound() {
        for in_process in [false, true] {
            let d = Dir::new();
            let i = d.ingest();
            let fake = Fake::default();
            let table = "world_traces";
            fake.fail(table, Fail::Undelivered);
            assert!(ingest_new_block(&fake, &i, &block(93)).await.is_err());
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            *fake.server_fence.borrow_mut() = Some(restart_after_client(t));
            if in_process {
                fake.processes.borrow_mut().insert(t.query_ids[0].clone());
            } else {
                fake.query_starts
                    .borrow_mut()
                    .insert(t.query_ids[0].clone());
            }
            fake.counts_asked.borrow_mut().clear();
            resume_stream_entries(&fake, &i).await.unwrap();
            let e = d.entry(&i);
            let t = e.tables.iter().find(|t| t.table == table).unwrap();
            assert_eq!(t.status, TableStatus::Parked);
            assert!(!t.sends_fenced());
            assert!(
                !fake
                    .counts_asked
                    .borrow()
                    .iter()
                    .any(|asked| asked == table),
                "the fence must precede even a zero row count of {table}, asked {:?}",
                fake.counts_asked.borrow()
            );
            assert_eq!(fake.tokens_sent(table).len(), 1);
            assert_eq!(fake.landed(LEDGER_TABLE), 0);
        }
    }

    #[tokio::test]
    async fn unavailable_absence_privilege_keeps_owner_and_never_counts_or_replays() {
        let d = Dir::new();
        let i = d.ingest();
        let fake = Fake::default();
        let table = "world_traces";
        fake.fail(table, Fail::Undelivered);
        assert!(ingest_new_block(&fake, &i, &block(94)).await.is_err());
        *fake.absence_error.borrow_mut() = Some("SYSTEM FLUSH LOGS access denied".into());
        fake.counts_asked.borrow_mut().clear();
        assert!(resume_stream_entries(&fake, &i)
            .await
            .unwrap_err()
            .contains("access denied"));
        let e = d.entry(&i);
        let t = e.tables.iter().find(|t| t.table == table).unwrap();
        assert!(!t.sends_fenced());
        assert_eq!(t.generation, 0);
        assert_eq!(fake.tokens_sent(table).len(), 1);
        assert!(fake.counts_asked.borrow().is_empty());
        assert_eq!(d.files(), ["94.block", "94.json"]);
    }
}
