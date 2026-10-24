//! Phase-2 F3/F4 — Rust port of Go RecoveryManager.HealBlock (RPC → raw CH cores).
//! Distinct from backfiller.rs (which only re-derives views from world_logs).
//!
//! Every core insert has a durable JSON owner, manifest, generation, and complete query_id history before network
//! I/O. A streamed insert can commit a prefix before the body ends, so a write error is not proof of noncommit.
//! Synchronous acknowledgements and positive terminal query_log records fence every prior send before a retry
//! or token change; a count alone never stops an older request committing later. Uncertain tables stay parked.
//! Lost-payload Stream/Derive owners retain their existing manifest and generations. JSON-only Heal owners resume
//! through the raw RPC path independently of queue rows, then release only after all sends are fenced and rows done.
//! The standalone CLI holds the same writer flock for its entire range and performs the same schema preflight.

use clickhouse::Client;
use prost::Message;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::ingest_journal::{
    self, BlockLocks, Entry, EntryKind, Ingest, Journal, Loaded, TableStatus, MAX_GAP_CHUNK,
};
use crate::nodestream::BlockNotification;
use crate::types::*;

const RETH_RPC: &str = "http://127.0.0.1:9745";
const CLICKHOUSE_URL: &str = "http://127.0.0.1:8123";
const CLICKHOUSE_DB: &str = "worldchain";
const HEALED_BY: &str = "rust-raw-heal";

/// CLI `--raw-heal` must NOT enqueue into `world_sequence_gaps` by default.
/// Enqueueing races the live `drain_sequence_gaps` healer (dual RPC/`trace_block` load → op-reth RSS).
/// Opt-in only: `TANK_RAW_HEAL_ENQUEUE=1`.
pub fn env_cli_raw_heal_enqueue() -> bool {
    matches!(
        std::env::var("TANK_RAW_HEAL_ENQUEUE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

/// Skip per-block `trace_block` / debug_trace during raw heal (cores-first; cuts archive RPC RAM pressure).
/// Enable: `RAW_HEAL_SKIP_TRACES=1`.
pub fn env_raw_heal_skip_traces() -> bool {
    matches!(
        std::env::var("RAW_HEAL_SKIP_TRACES").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

/// CLI `--raw-heal` visits every block in its range, present or not, and writes only the core tables the block has
/// no rows in: for blocks whose block row and transactions were written but whose logs or traces were lost.
/// Enable: `RAW_HEAL_REFILL_PRESENT=1`.
pub fn env_raw_heal_refill_present() -> bool {
    matches!(
        std::env::var("RAW_HEAL_REFILL_PRESENT").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

/// Live healer: disable sequence/raw drain (CLI single-writer window).
/// Enable: `TANK_DISABLE_SEQUENCE_DRAIN=1`.
pub fn env_disable_sequence_drain() -> bool {
    matches!(
        std::env::var("TANK_DISABLE_SEQUENCE_DRAIN").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

#[derive(Debug, Clone, clickhouse::Row, serde::Serialize)]
pub struct SequenceGapRow {
    pub gap_lo: u64,
    pub gap_hi: u64,
    pub detected_at: u64,
    pub resolved: u8,
    pub source: String,
}

pub async fn ensure_sequence_gaps_table(
    ch: &Client,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Idempotent DDL — safe on every healer start.
    ch.query(
        r#"
        CREATE TABLE IF NOT EXISTS worldchain.world_sequence_gaps
        (
            gap_lo UInt64,
            gap_hi UInt64,
            detected_at DateTime64(3),
            resolved UInt8,
            source LowCardinality(String)
        )
        ENGINE = ReplacingMergeTree(detected_at)
        ORDER BY (gap_lo, gap_hi)
        "#,
    )
    .execute()
    .await?;
    Ok(())
}

/// Queues [gap_lo, gap_hi] whole, in rows of at most MAX_GAP_CHUNK heights (one insert). The old version kept only the
/// newest 10,000 heights of a longer gap and dropped the rest.
pub async fn enqueue_sequence_gap(
    ch: &Client,
    gap_lo: u64,
    gap_hi: u64,
    source: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let chunks = ingest_journal::gap_chunks(gap_lo, gap_hi);
    enqueue_gap_rows(ch, &chunks, ingest_journal::wall_ms(), source).await?;
    if !chunks.is_empty() {
        info!(
            "TANK-ENGINE enqueued sequence gap [{}, {}] in {} row(s) source={}",
            gap_lo,
            gap_hi,
            chunks.len(),
            source
        );
    }
    Ok(())
}

/// One synchronous insert of unresolved queue rows. `detected_at_ms` is the rows' version in world_sequence_gaps
/// (ReplacingMergeTree(detected_at)): a row queued again with the same value never reopens a row healed since.
pub async fn enqueue_gap_rows(
    ch: &Client,
    chunks: &[(u64, u64)],
    detected_at_ms: u64,
    source: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if chunks.is_empty() {
        return Ok(());
    }
    if let Some((lo, hi)) = chunks
        .iter()
        .find(|(lo, hi)| hi < lo || hi - lo >= MAX_GAP_CHUNK)
    {
        return Err(format!(
            "queue row [{lo}, {hi}] is empty or longer than {MAX_GAP_CHUNK} heights"
        )
        .into());
    }
    let mut insert = ch
        .insert("world_sequence_gaps")?
        .with_option("async_insert", "0");
    for (lo, hi) in chunks {
        insert
            .write(&SequenceGapRow {
                gap_lo: *lo,
                gap_hi: *hi,
                detected_at: detected_at_ms,
                resolved: 0,
                source: source.to_string(),
            })
            .await?;
    }
    insert.end().await?;
    Ok(())
}

/// Rows one block already has in each core table. Every writer puts a table's rows of one block in one insert, so a
/// table holds all of a block's rows or none: a heal writes a table only when it holds none (nothing twice).
#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct StoredRows {
    blocks: u64,
    tx_count: u64, // transactions_count of the block row, 0 without one
    txs: u64,
    logs: u64,
    traces: u64,
}

impl StoredRows {
    /// The block row and, when the block has transactions, at least one of them. A bare block row (left by a heal
    /// that failed after writing it first, before 2026-10-13) counts as missing, so a heal fills it in.
    /// missing_heights applies the same rule in SQL, to a whole range at once.
    #[cfg_attr(not(test), allow(dead_code))]
    fn present(&self) -> bool {
        self.blocks > 0 && (self.tx_count == 0 || self.txs > 0)
    }

    /// Nothing a heal could add: every core table holds the block (traces only when traces are healed).
    fn complete(&self, skip_traces: bool) -> bool {
        self.blocks > 0 && self.txs > 0 && self.logs > 0 && (self.traces > 0 || skip_traces)
    }
}

async fn stored_rows(
    ch: &Client,
    b: u64,
) -> Result<StoredRows, Box<dyn std::error::Error + Send + Sync>> {
    // ClickHouse types every scalar subquery Nullable; RowBinary then sends a null-flag byte before each value, which
    // a u64 field misreads (shifted bytes, no error). ifNull makes each column a plain UInt64.
    // A failed count must stop the heal of this block, never read as "nothing stored" (that would write twice).
    Ok(ch
        .query(&format!(
            "SELECT \
             toUInt64(ifNull((SELECT count() FROM worldchain.world_blocks WHERE chain_id = 480 AND block_number = {0}), 0)) AS blocks, \
             toUInt64(ifNull((SELECT max(transactions_count) FROM worldchain.world_blocks WHERE chain_id = 480 AND block_number = {0}), 0)) AS tx_count, \
             toUInt64(ifNull((SELECT count() FROM worldchain.world_transactions WHERE chain_id = 480 AND block_number = {0}), 0)) AS txs, \
             toUInt64(ifNull((SELECT count() FROM worldchain.world_logs WHERE chain_id = 480 AND block_number = {0}), 0)) AS logs, \
             toUInt64(ifNull((SELECT count() FROM worldchain.world_traces WHERE chain_id = 480 AND block_number = {0}), 0)) AS traces",
            b
        ))
        .fetch_one()
        .await?)
}

/// Heights in [lo, hi] that are not present (StoredRows::present, in SQL), in order: one query for a whole range,
/// where a check per height costs ~20 ms (a 10,000-block gap: ~200 s on every drain tick).
async fn missing_heights(
    ch: &Client,
    lo: u64,
    hi: u64,
) -> Result<Vec<u64>, Box<dyn std::error::Error + Send + Sync>> {
    // a named row: a bare u64 Row fetch_all silently yields an empty Vec (healer.rs GapBn)
    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct Height {
        b: u64,
    }
    if hi < lo {
        return Ok(Vec::new());
    }
    let rows: Vec<Height> = ch
        .query(&format!(
            "SELECT number AS b FROM numbers({0}, {2}) WHERE number NOT IN ( \
             SELECT block_number FROM worldchain.world_blocks WHERE chain_id = 480 AND block_number BETWEEN {0} AND {1} \
             GROUP BY block_number HAVING max(transactions_count) = 0 OR block_number IN ( \
             SELECT block_number FROM worldchain.world_transactions WHERE chain_id = 480 AND block_number BETWEEN {0} AND {1})) \
             ORDER BY b",
            lo,
            hi,
            hi - lo + 1
        ))
        .fetch_all()
        .await?;
    Ok(rows.into_iter().map(|r| r.b).collect())
}

/// Heights per presence query in a CLI range walk.
const PRESENCE_CHUNK: u64 = 10_000;

/// CLI / one-shot: heal every missing height in [start, end] via archive RPC.
pub async fn run_raw_heal_range(
    start_block: u64,
    end_block: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(
        "=== TANK-ENGINE RAW-HEAL MODE (RPC→cores) | {} -> {} ===",
        start_block, end_block
    );
    let journal = Journal::open(
        Path::new(ingest_journal::JOURNAL_DIR),
        Path::new(ingest_journal::WRITER_LOCK),
    )?;
    let locks = BlockLocks::new();
    let ch = Client::default()
        .with_url(CLICKHOUSE_URL)
        .with_database(CLICKHOUSE_DB);
    crate::streamer::check_schema(&ch).await?;
    ensure_sequence_gaps_table(&ch).await?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    // Single-writer default: CLI walks alone. Enqueue only if explicitly opted in
    // (otherwise live drain_sequence_gaps races the same range → dual trace_block load).
    if env_cli_raw_heal_enqueue() {
        warn!(
            "TANK-ENGINE RAW-HEAL enqueue ENABLED (TANK_RAW_HEAL_ENQUEUE) — ensure live sequence drain is disabled or idle"
        );
        enqueue_sequence_gap(&ch, start_block, end_block, "cli-raw-heal").await?;
    } else {
        info!(
            "TANK-ENGINE RAW-HEAL single-writer mode (no world_sequence_gaps enqueue); set TANK_RAW_HEAL_ENQUEUE=1 to opt in"
        );
    }
    if env_raw_heal_skip_traces() {
        info!("TANK-ENGINE RAW-HEAL RAW_HEAL_SKIP_TRACES=1 — cores only (no trace_block)");
    }
    let refill = env_raw_heal_refill_present();
    if refill {
        info!("TANK-ENGINE RAW-HEAL RAW_HEAL_REFILL_PRESENT=1 — every block in range, only its empty core tables");
    }

    let mut healed = 0u64;
    let mut failed = 0u64;
    let mut owned = 0u64;
    let mut lo = start_block;
    loop {
        let hi = end_block.min(lo.saturating_add(PRESENCE_CHUNK - 1));
        // Skip present blocks (idempotent); REFILL_PRESENT visits every block. If the presence query fails, every
        // block of the chunk is visited: heal_block_rpc re-reads what each one holds and writes only what is missing.
        let mut heights: Vec<u64> = if refill {
            (lo..=hi).collect()
        } else {
            match missing_heights(&ch, lo, hi).await {
                Ok(m) => m,
                Err(e) => {
                    warn!("TANK-ENGINE raw-heal presence query failed for [{}, {}]: {} — visiting every block", lo, hi, e);
                    (lo..=hi).collect()
                }
            }
        };
        // Saved Heal owners are revisited even if an earlier attempt made the presence query look complete.
        for item in journal.load(EntryKind::Heal)? {
            if let Loaded::Entry(entry) = item {
                if (lo..=hi).contains(&entry.block_number) {
                    heights.push(entry.block_number);
                }
            }
        }
        heights.sort_unstable();
        heights.dedup();
        for b in heights {
            match heal_block(&ch, &http, b, &journal, &locks).await {
                Ok(HealOutcome::Healed) => {
                    healed += 1;
                    if healed % 50 == 0 {
                        info!("TANK-ENGINE raw-heal progress healed={} last={}", healed, b);
                    }
                }
                Ok(HealOutcome::Owned | HealOutcome::RetiringStream) => {
                    owned += 1;
                    warn!("TANK-ENGINE raw-heal block={} skipped: an existing Stream/Derive owner or orphan payload retains it", b);
                }
                Err(e) => {
                    failed += 1;
                    warn!("TANK-ENGINE raw-heal failed block={}: {}", b, e);
                }
            }
        }
        if hi >= end_block {
            break;
        }
        lo = hi + 1;
    }
    info!(
        "TANK-ENGINE raw-heal complete healed={} failed={} journal_owned={} range=[{}, {}]",
        healed, failed, owned, start_block, end_block
    );
    Ok(())
}

/// What a heal of one height came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealOutcome {
    /// The block's core tables hold its rows (written now or found stored).
    Healed,
    /// The ingest journal owns the block: its entry writes the core tables.
    Owned,
    /// An unsent, unparked Stream payload is being retired to its claimed gap without writing rows.
    RetiringStream,
}

/// What the drain does with one queue row after walking it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RowDecision {
    /// Every missing height is healed or queued again as its own row: mark the row resolved.
    Resolve { requeue: Vec<u64> },
    /// Heights are left (budget spent, or a one-block row failed or is still retiring): keep the row unresolved.
    Keep,
}

/// The drain's rule for one row [lo, hi] whose presence query found `missing` heights, of which the first
/// `attempted.len()` were attempted in order. An owner normally finishes its height; only an unsent Stream retiring
/// without writing must retain its recovery row. Never resolves a height that was neither handled nor queued again.
pub(crate) fn decide_row(
    lo: u64,
    hi: u64,
    missing: usize,
    attempted: &[(u64, Option<HealOutcome>)],
) -> RowDecision {
    if hi < lo || hi - lo >= MAX_GAP_CHUNK || attempted.len() < missing {
        return RowDecision::Keep;
    }
    let failed: Vec<u64> = attempted
        .iter()
        .filter(|(_, outcome)| matches!(outcome, None | Some(HealOutcome::RetiringStream)))
        .map(|(b, _)| *b)
        .collect();
    if lo == hi && !failed.is_empty() {
        return RowDecision::Keep;
    }
    RowDecision::Resolve { requeue: failed }
}

pub async fn drain_sequence_gaps(
    ch: &Client,
    http: &reqwest::Client,
    max_blocks: u64,
    ingest: &Ingest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_sequence_gaps_table(ch).await?;
    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct Gap {
        gap_lo: u64,
        gap_hi: u64,
    }
    // A row above MAX_GAP_CHUNK heights is never walked (a bogus [1, tip] row would write every cold month: the
    // 2026-10-12 parts outage) and never resolved: it stays queued for the operator, reported every tick. Producers
    // queue gaps in rows of at most MAX_GAP_CHUNK heights.
    let oversized: Vec<Gap> = ch
        .query(&format!(
            "SELECT gap_lo, gap_hi FROM worldchain.world_sequence_gaps FINAL \
             WHERE resolved = 0 AND gap_hi - gap_lo >= {MAX_GAP_CHUNK} ORDER BY gap_lo ASC LIMIT 5"
        ))
        .fetch_all()
        .await?;
    for g in &oversized {
        error!(
            "TANK-ENGINE INGEST_GAP_OVERSIZED sequence gap [{}, {}] spans {} heights (> {}): never walked or resolved automatically; split it into rows of at most {} heights or resolve it by hand",
            g.gap_lo,
            g.gap_hi,
            g.gap_hi - g.gap_lo + 1,
            MAX_GAP_CHUNK,
            MAX_GAP_CHUNK
        );
    }
    let gaps: Vec<Gap> = ch
        .query(&format!(
            "SELECT gap_lo, gap_hi FROM worldchain.world_sequence_gaps FINAL \
             WHERE resolved = 0 AND gap_hi >= gap_lo AND gap_hi - gap_lo < {MAX_GAP_CHUNK} ORDER BY gap_lo ASC LIMIT 20"
        ))
        .fetch_all()
        .await?;

    let mut budget = max_blocks;
    for g in gaps {
        // one presence query per gap; a failed query leaves the gap queued for the next tick
        let missing = match missing_heights(ch, g.gap_lo, g.gap_hi).await {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    "sequence gap [{}, {}] presence query failed: {} (stays queued)",
                    g.gap_lo, g.gap_hi, e
                );
                continue;
            }
        };
        let mut attempted: Vec<(u64, Option<HealOutcome>)> = Vec::new();
        for &b in &missing {
            if budget == 0 {
                break;
            }
            budget -= 1;
            match heal_block(ch, http, b, &ingest.journal, &ingest.locks).await {
                Ok(outcome) => attempted.push((b, Some(outcome))),
                Err(e) => {
                    warn!("sequence heal block {} err: {} (stays queued)", b, e);
                    attempted.push((b, None));
                }
            }
        }
        // A wider row queues each failed block again as its own one-block row before it is marked resolved, so a later
        // tick retries just those blocks; a one-block row that failed stays unresolved.
        if let RowDecision::Resolve { requeue } =
            decide_row(g.gap_lo, g.gap_hi, missing.len(), &attempted)
        {
            let rows: Vec<(u64, u64)> = requeue.iter().map(|b| (*b, *b)).collect();
            enqueue_gap_rows(ch, &rows, ingest_journal::wall_ms(), "heal-failed-retry").await?;
            let mut insert = ch
                .insert("world_sequence_gaps")?
                .with_option("async_insert", "0");
            insert
                .write(&SequenceGapRow {
                    gap_lo: g.gap_lo,
                    gap_hi: g.gap_hi,
                    detected_at: ingest_journal::wall_ms(),
                    resolved: 1,
                    source: "healer-resolved".to_string(),
                })
                .await?;
            insert.end().await?;
        }
        if budget == 0 {
            break;
        }
    }
    Ok(())
}

/// Heals one height under the shared block lock. Existing payload-bearing owners and orphan payloads
/// stay untouched; Heal owners and prior lost-payload repair owners use their saved state.
pub(crate) async fn heal_block(
    ch: &Client,
    http: &reqwest::Client,
    b: u64,
    journal: &Journal,
    locks: &Arc<BlockLocks>,
) -> Result<HealOutcome, Box<dyn std::error::Error + Send + Sync>> {
    let _owner = locks.lock(b).await;
    let entry = if journal.owns(b) {
        let Some(entry) = journal.entry(b)? else {
            return Ok(HealOutcome::Owned);
        };
        if entry.kind != EntryKind::Heal {
            if let Some(payload) = journal.entry_payload(&entry)? {
                if entry.kind == EntryKind::Stream
                    && crate::streamer::stream_entry_is_unsent(&entry)
                    && !entry.tables.iter().any(|t| t.status == TableStatus::Parked)
                    && BlockNotification::decode(payload.bytes.as_slice())
                        .ok()
                        .is_some_and(|block| {
                            block.number == b
                                && entry.block_hash.eq_ignore_ascii_case(block.hash.trim())
                                && crate::streamer::stream_missing_traces(&block)
                        })
                {
                    return Ok(HealOutcome::RetiringStream);
                }
                return Ok(HealOutcome::Owned);
            }
        }
        Some(entry)
    } else {
        None
    };
    heal_block_rpc(ch, http, b, journal, entry).await?;
    Ok(HealOutcome::Healed)
}

/// Independent of sequence/raw draining: a queue may already be resolved while its old sends are still unknown.
pub(crate) async fn resume_heal_entries(
    ch: &Client,
    http: &reqwest::Client,
    ingest: &Ingest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for item in ingest.journal.load(EntryKind::Heal)? {
        let Loaded::Entry(entry) = item else {
            continue;
        };
        let b = entry.block_number;
        if let Err(e) = heal_block(ch, http, b, &ingest.journal, &ingest.locks).await {
            warn!("TANK-ENGINE Heal owner block={} retained: {}", b, e);
        }
    }
    Ok(())
}

async fn heal_block_rpc(
    ch: &Client,
    http: &reqwest::Client,
    b_num: u64,
    journal: &Journal,
    mut saved_entry: Option<Entry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(entry) = saved_entry.as_mut() {
        refresh_core_states(ch, journal, entry).await?;
        if entry.kind == EntryKind::Heal && entry.heal_complete {
            // Zero-log/zero-transaction blocks need no archive RPC once the full path was durably completed.
            finish_heal(journal, entry)?;
            return Ok(());
        }
    }
    let stored = stored_rows(ch, b_num).await?;
    // A previously recorded trace send must be resumed even when new trace work is disabled.
    let skip_traces = env_raw_heal_skip_traces()
        && !saved_entry.as_ref().is_some_and(|entry| {
            entry
                .tables
                .iter()
                .any(|t| t.table == "world_traces" && t.status != TableStatus::Done)
        });
    if stored.complete(skip_traces)
        && !saved_entry
            .as_ref()
            .is_some_and(|entry| entry.kind == EntryKind::Heal)
    {
        return Ok(());
    }
    let hex_num = format!("0x{:x}", b_num);
    let block_body = rpc_call(http, "eth_getBlockByNumber", json!([hex_num, true])).await?;
    let rb = block_body
        .get("result")
        .cloned()
        .ok_or("eth_getBlockByNumber missing result")?;
    if rb.is_null() {
        return Err(format!("block {} not found on archive RPC", b_num).into());
    }
    let b_hash = rb
        .get("hash")
        .and_then(|v| v.as_str())
        .ok_or("missing hash")?;
    let parent = rb.get("parentHash").and_then(|v| v.as_str()).unwrap_or("");
    let miner = rb.get("miner").and_then(|v| v.as_str()).unwrap_or("0x0");
    let ts_hex = rb
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    let block_ts = decode_hex_u64(ts_hex);
    if block_ts == 0 {
        // never write timestamp 0: every core table partitions by month, 0 is partition 197001
        return Err(format!("block {} has no timestamp on archive RPC", b_num).into());
    }
    let gas_limit = decode_hex_u64(rb.get("gasLimit").and_then(|v| v.as_str()).unwrap_or("0x0"));
    let gas_used = decode_hex_u64(rb.get("gasUsed").and_then(|v| v.as_str()).unwrap_or("0x0"));
    let base_fee = decode_hex_u64(
        rb.get("baseFeePerGas")
            .and_then(|v| v.as_str())
            .unwrap_or("0x0"),
    );
    let size_bytes =
        decode_hex_u64(rb.get("size").and_then(|v| v.as_str()).unwrap_or("0x0")) as u32;
    let txs = rb
        .get("transactions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut entry = match saved_entry {
        Some(entry) => {
            if !entry.block_hash.eq_ignore_ascii_case(b_hash.trim())
                || entry.block_timestamp != block_ts
            {
                return Err(
                    format!("block {b_num}: RPC identity differs from its journal owner").into(),
                );
            }
            entry
        }
        None => journal.create_heal(b_num, b_hash, block_ts, ingest_journal::wall_ms())?,
    };
    let now_ms = entry.now_ms;

    let block_row = BlockRow {
        chain_id: 480,
        block_number: b_num,
        block_hash: str_to_fixed_66(b_hash),
        parent_hash: str_to_fixed_66(parent),
        timestamp: block_ts * 1000,
        miner: str_to_fixed_42(miner),
        gas_limit,
        gas_used,
        base_fee_per_gas: base_fee,
        transactions_count: txs.len() as u32,
        size_bytes,
        ingested_at: now_ms,
        is_canonical: 1,
        healed_by: HEALED_BY.to_string(),
    };
    // The block row is written last (below): it is what every heal checks for presence. Written first, a heal that
    // failed after it left a block with no transactions, logs or traces that no heal revisited (897 such blocks in
    // 202607-202610, found 2026-10-13).

    // Receipts (batch then per-tx fallback)
    let receipts_body = rpc_call(http, "eth_getBlockReceipts", json!([hex_num])).await?;
    let mut receipts = receipts_body
        .get("result")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut rcp_by_hash = std::collections::HashMap::new();
    for r in &receipts {
        if let Some(h) = r.get("transactionHash").and_then(|v| v.as_str()) {
            rcp_by_hash.insert(h.to_lowercase(), r.clone());
        }
    }
    if rcp_by_hash.len() < txs.len() {
        for tx in &txs {
            let h = tx.get("hash").and_then(|v| v.as_str()).unwrap_or("");
            if h.is_empty() || rcp_by_hash.contains_key(&h.to_lowercase()) {
                continue;
            }
            if let Ok(body) = rpc_call(http, "eth_getTransactionReceipt", json!([h])).await {
                if let Some(r) = body.get("result").cloned() {
                    if !r.is_null() {
                        rcp_by_hash.insert(h.to_lowercase(), r.clone());
                        receipts.push(r);
                    }
                }
            }
        }
    }
    // rpc_call returns Ok for a JSON-RPC error body, so receipts can be missing here. A transaction without its
    // receipt would be stored with status, gas_used and l1 fee 0 and without its logs, and the per-table presence
    // check would then call the block whole for good: fail the block instead (nothing written, retried later).
    if rcp_by_hash.len() < txs.len() {
        return Err(format!(
            "block {} has {} of {} receipts",
            b_num,
            rcp_by_hash.len(),
            txs.len()
        )
        .into());
    }

    let mut tx_rows = Vec::new();
    for (idx, tx) in txs.iter().enumerate() {
        let th = tx.get("hash").and_then(|v| v.as_str()).unwrap_or("");
        let rcp = rcp_by_hash.get(&th.to_lowercase());
        let status = rcp
            .and_then(|r| r.get("status"))
            .and_then(|v| v.as_str())
            .map(|s| if decode_hex_u64(s) == 1 { 1u8 } else { 0u8 })
            .unwrap_or(0);
        let gas_used_tx = rcp
            .and_then(|r| r.get("gasUsed"))
            .and_then(|v| v.as_str())
            .map(decode_hex_u64)
            .unwrap_or(0);
        let tx_type =
            decode_hex_u64(tx.get("type").and_then(|v| v.as_str()).unwrap_or("0x0")) as u8;
        let is_system = if tx_type == 126 { 1u8 } else { 0u8 };
        let l1_fee = rcp
            .and_then(|r| r.get("l1Fee"))
            .and_then(|v| v.as_str())
            .unwrap_or("0x0");
        tx_rows.push(TransactionRow {
            chain_id: 480,
            block_number: b_num,
            block_hash: str_to_fixed_66(b_hash),
            tx_index: idx as u32,
            tx_hash: str_to_fixed_66(th),
            from_address: str_to_fixed_42(tx.get("from").and_then(|v| v.as_str()).unwrap_or("")),
            to_address: str_to_fixed_42(tx.get("to").and_then(|v| v.as_str()).unwrap_or("")),
            value_wei: parse_u256_bytes(tx.get("value").and_then(|v| v.as_str()).unwrap_or("0x0")),
            mint_wei: parse_u256_bytes("0x0"),
            gas_price_wei: decode_hex_u64(
                tx.get("gasPrice").and_then(|v| v.as_str()).unwrap_or("0x0"),
            ),
            max_fee_per_gas_wei: decode_hex_u64(
                tx.get("maxFeePerGas")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0x0"),
            ),
            max_priority_fee_wei: decode_hex_u64(
                tx.get("maxPriorityFeePerGas")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0x0"),
            ),
            gas_limit: decode_hex_u64(tx.get("gas").and_then(|v| v.as_str()).unwrap_or("0x0")),
            gas_used: gas_used_tx,
            nonce: decode_hex_u64(tx.get("nonce").and_then(|v| v.as_str()).unwrap_or("0x0")),
            tx_type,
            is_system_tx: is_system,
            input_data: tx
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("0x")
                .to_string(),
            status,
            effective_gas_price_wei: decode_hex_u64(
                tx.get("gasPrice").and_then(|v| v.as_str()).unwrap_or("0x0"),
            ),
            ingested_at: now_ms,
            block_timestamp: block_ts as u32,
            l1_fee_wei: parse_u256_bytes(l1_fee),
            l1_gas_price_wei: 0,
            is_canonical: 1,
            healed_by: HEALED_BY.to_string(),
        });
    }
    let mut wrote_txs = 0usize;
    if stored.txs == 0 && !tx_rows.is_empty() {
        insert_block_rows(
            ch,
            "world_transactions",
            &tx_rows,
            b_num,
            b_hash,
            journal,
            &mut entry,
        )
        .await?;
        wrote_txs = tx_rows.len();
    }

    let mut log_rows = Vec::new();
    for r in &receipts {
        let th = r
            .get("transactionHash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tx_index = decode_hex_u64(
            r.get("transactionIndex")
                .and_then(|v| v.as_str())
                .unwrap_or("0x0"),
        ) as u32;
        if let Some(logs) = r.get("logs").and_then(|v| v.as_array()) {
            for log in logs {
                let topics: Vec<FixedString66> = log
                    .get("topics")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| t.as_str().map(str_to_fixed_66))
                            .collect()
                    })
                    .unwrap_or_default();
                log_rows.push(LogRow {
                    chain_id: 480,
                    block_number: b_num,
                    block_hash: str_to_fixed_66(b_hash),
                    tx_hash: str_to_fixed_66(th),
                    tx_index,
                    log_index: decode_hex_u64(
                        log.get("logIndex")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0x0"),
                    ) as u32,
                    address: str_to_fixed_42(
                        log.get("address").and_then(|v| v.as_str()).unwrap_or(""),
                    ),
                    data: log
                        .get("data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0x")
                        .to_string(),
                    topics,
                    ingested_at: now_ms,
                    block_timestamp: block_ts as u32,
                    is_canonical: 1,
                    healed_by: HEALED_BY.to_string(),
                });
            }
        }
    }
    let mut wrote_logs = 0usize;
    if stored.logs == 0 && !log_rows.is_empty() {
        insert_block_rows(
            ch,
            "world_logs",
            &log_rows,
            b_num,
            b_hash,
            journal,
            &mut entry,
        )
        .await?;
        wrote_logs = log_rows.len();
    }

    // Best-effort traces (same as Go: trace_block; fallback debug_traceBlockByNumber).
    // Skip under RAW_HEAL_SKIP_TRACES=1 to reduce op-reth working-set pressure during bulk hole fill.
    if skip_traces {
        if stored.blocks == 0 {
            insert_block_rows(
                ch,
                "world_blocks",
                std::slice::from_ref(&block_row),
                b_num,
                b_hash,
                journal,
                &mut entry,
            )
            .await?;
        }
        complete_heal(journal, &mut entry)?;
        info!(
            "TANK-ENGINE raw-healed block={} wrote txs={} logs={} block_row={} (traces skipped)",
            b_num,
            wrote_txs,
            wrote_logs,
            stored.blocks == 0
        );
        return Ok(());
    }
    let mut traced = 0usize;
    if stored.traces == 0 {
        if let Ok(tbody) = rpc_call(http, "trace_block", json!([hex_num])).await {
            if let Some(arr) = tbody.get("result").and_then(|v| v.as_array()) {
                let traces =
                    trace_rows_from_trace_block(arr, b_num, b_hash, block_ts, now_ms, HEALED_BY);
                if !traces.is_empty()
                    && !crate::streamer::trace_coverage_missing(
                        tx_rows.iter().map(|tx| tx.tx_index),
                        |index| traces.iter().any(|trace| trace.tx_index == index),
                    )
                {
                    insert_block_rows(
                        ch,
                        "world_traces",
                        &traces,
                        b_num,
                        b_hash,
                        journal,
                        &mut entry,
                    )
                    .await?;
                    traced = traces.len();
                }
            }
        }
    }
    if stored.traces == 0 && traced == 0 && block_row.transactions_count > 0 {
        // A block with transactions always has traces for every transaction. Reject incomplete RPC coverage before
        // any trace insert, and queue the whole block for the trace-gap healer instead.
        insert_queue_row(
            ch,
            "world_trace_gaps",
            &crate::healer::TraceGapRow {
                block_number: b_num,
                rpc_error_code: 0,
                reason: "raw-heal-no-traces".to_string(),
                attempted_at: now_ms,
                resolved: 0,
                resolved_at: 0,
            },
        )
        .await?;
        warn!(
            "TANK-ENGINE raw-heal block={} missing transaction traces; queued for the trace-gap healer",
            b_num
        );
    }
    if stored.blocks == 0 {
        insert_block_rows(
            ch,
            "world_blocks",
            std::slice::from_ref(&block_row),
            b_num,
            b_hash,
            journal,
            &mut entry,
        )
        .await?;
    }
    complete_heal(journal, &mut entry)?;

    info!(
        "TANK-ENGINE raw-healed block={} wrote txs={} logs={} traces={} block_row={}",
        b_num,
        wrote_txs,
        wrote_logs,
        traced,
        stored.blocks == 0
    );
    Ok(())
}

/// trace_block (parity) rows at raw-heal fidelity: tx hash, gas, callType, depth from traceAddress, the block's
/// hash and timestamp, one block-wide trace_index. Shared with the trace-gap healer (healer.rs), whose callTracer
/// rows had block_timestamp 0, no hashes and no gas until 2026-10-12 (partition 197001 incident).
pub(crate) fn trace_rows_from_trace_block(
    arr: &[Value],
    b_num: u64,
    b_hash: &str,
    block_ts: u64,
    now_ms: u64,
    healed_by: &str,
) -> Vec<TraceRow> {
    let mut traces = Vec::new();
    let mut idx = 0u32;
    for entry in arr {
        let action = entry.get("action").cloned().unwrap_or(json!({}));
        let result = entry.get("result").cloned().unwrap_or(json!({}));
        let tx_hash = entry
            .get("transactionHash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tx_index = entry
            .get("transactionPosition")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        traces.push(TraceRow {
            chain_id: 480,
            block_number: b_num,
            block_hash: str_to_fixed_66(b_hash),
            tx_index,
            tx_hash: str_to_fixed_66(tx_hash),
            from_address: str_to_fixed_42(
                action.get("from").and_then(|v| v.as_str()).unwrap_or(""),
            ),
            to_address: str_to_fixed_42(action.get("to").and_then(|v| v.as_str()).unwrap_or("")),
            value_wei: parse_u256_bytes(
                action
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0x0"),
            ),
            input_data: action
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            output_data: result
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            gas_limit: decode_hex_u64(action.get("gas").and_then(|v| v.as_str()).unwrap_or("0x0")),
            gas_used: decode_hex_u64(
                result
                    .get("gasUsed")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0x0"),
            ),
            trace_type: entry
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("call")
                .to_string(),
            call_type: action
                .get("callType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            error: entry
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            depth: entry
                .get("traceAddress")
                .and_then(|v| v.as_array())
                .map(|a| a.len() as u32)
                .unwrap_or(0),
            ingested_at: now_ms,
            block_timestamp: block_ts as u32,
            is_canonical: 1,
            trace_index: idx,
            healed_by: healed_by.to_string(),
        });
        idx += 1;
    }
    traces
}

/// Hash and timestamp of a block from the archive RPC. Err only when the RPC call itself fails (the caller aborts its
/// run, as before); Ok(None) when the block, its hash or its timestamp is missing: a missing timestamp must never
/// become 0 (the rows would land in partition 197001).
pub(crate) async fn block_hash_and_ts(
    http: &reqwest::Client,
    b_num: u64,
) -> Result<Option<(String, u64)>, Box<dyn std::error::Error + Send + Sync>> {
    let body = rpc_call(
        http,
        "eth_getBlockByNumber",
        json!([format!("0x{:x}", b_num), false]),
    )
    .await?;
    let rb = match body.get("result") {
        Some(v) if !v.is_null() => v,
        _ => return Ok(None),
    };
    let hash = match rb.get("hash").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return Ok(None),
    };
    let ts = decode_hex_u64(
        rb.get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("0x0"),
    );
    if ts == 0 {
        return Ok(None);
    }
    Ok(Some((hash, ts)))
}

pub(crate) async fn rpc_call(
    http: &reqwest::Client,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let payload = json!({"jsonrpc":"2.0","method":method,"params":params,"id":1});
    let resp = http.post(RETH_RPC).json(&payload).send().await?;
    Ok(resp.json().await?)
}

fn decode_hex_u64(s: &str) -> u64 {
    let clean = s.trim_start_matches("0x");
    u64::from_str_radix(clean, 16).unwrap_or(0)
}

/// Existing recovery owners never invent a missing generation.
fn core_generation(entry: &Entry, table: &str) -> Result<u32, String> {
    entry
        .tables
        .iter()
        .find(|t| t.table == table)
        .map(|t| t.generation)
        .ok_or_else(|| {
            format!(
            "block {} table {table}: core recovery has no journal generation; entry kept unchanged",
            entry.block_number
        )
        })
}

/// Refuses a send the current state does not permit, before any network I/O. Done/Parked states are decisions, not
/// permission to reconstruct missing rows by replaying them.
fn check_core_send(
    entry: &Entry,
    table: &str,
    rows: usize,
    generation: u32,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let i = entry
        .tables
        .iter()
        .position(|t| t.table == table)
        .ok_or_else(|| {
            format!(
                "block {} table {table}: core recovery has no journal state",
                entry.block_number
            )
        })?;
    let t = &entry.tables[i];
    if t.status != TableStatus::Pending
        || t.generation != generation
        || t.rows != rows as u64
        || !t.sends_fenced()
    {
        return Err(format!(
            "block {} table {table}: core recovery refuses status={:?} generation={} rows={} (requested generation={generation} rows={rows}); journal state kept",
            entry.block_number, t.status, t.generation, t.rows
        ).into());
    }
    Ok(i)
}

/// Records the current generation's send and makes it durable before any request byte. Other table states and this
/// generation's earlier query_ids survive a failed save. A failed save does not leave an unsent id behind unless
/// removing that id itself fails, and it never releases the owner.
fn record_core_send(
    journal: &Journal,
    entry: &mut Entry,
    table: &str,
    rows: usize,
    generation: u32,
    query_id: &str,
    now_ms: u64,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let i = check_core_send(entry, table, rows, generation)?;
    let prior_first_send_ms = entry.tables[i].first_send_ms;
    entry.tables[i].record_send(query_id, now_ms)?;
    if let Err(err) = journal.save(entry) {
        crate::streamer::revert_unsent_send(journal, entry, i, query_id, prior_first_send_ms)?;
        return Err(format!(
            "block {} table {table}: journal save failed before the send: {err}; owner kept, nothing inserted",
            entry.block_number
        )
        .into());
    }
    Ok(i)
}

/// A generation changes only after all its sends are terminal and the global core count is zero.
/// Persist even the last attempted bump, so no later retry falls back to an earlier token.
fn advance_core_generation(
    journal: &Journal,
    entry: &mut Entry,
    i: usize,
) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
    let generation = entry.tables[i].next_generation()?;
    journal.save(entry)?;
    Ok(generation)
}

/// Persist terminal evidence before consulting row counts or allowing any replay. A failed evidence lookup is an
/// error to retry, never a decision. An unfenced table waits (Err, nothing sent) for the journal's fence wait, then
/// parks.
async fn fence_core_sends(
    ch: &Client,
    journal: &Journal,
    entry: &mut Entry,
    i: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target = crate::streamer::ClickHouseTarget::new(ch.clone());
    let fenced = crate::streamer::fence_sends(&target, journal, entry, i).await?;
    if fenced {
        return Ok(());
    }
    if let Some(left) = journal.fence_wait_left(&entry.tables[i], ingest_journal::wall_ms()) {
        return Err(crate::streamer::fence_wait_error(
            entry.block_number,
            &entry.tables[i].table,
            left,
        )
        .into());
    }
    let why = format!(
        "block {} table {}: prior sends are not all terminal after the {}s fence wait; no replay or owner release",
        entry.block_number,
        entry.tables[i].table,
        journal.fence_wait_ms() / 1000
    );
    entry.tables[i].status = TableStatus::Parked;
    entry.tables[i].last_error = Some(why.clone());
    entry.tables[i].note = Some(why.clone());
    journal.save(entry)?;
    Err(why.into())
}

async fn core_count(
    ch: &Client,
    table: &str,
    block_number: u64,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    Ok(ch.query(&format!(
        "SELECT toUInt64(count()) AS n FROM {CLICKHOUSE_DB}.{table} WHERE chain_id = 480 AND block_number = {block_number}"
    )).fetch_one::<Count>().await?.n)
}

/// Reconciles even tables whose rows have appeared since a failed send. Positive counts never fence a request.
pub(crate) async fn refresh_core_states(
    ch: &Client,
    journal: &Journal,
    entry: &mut Entry,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for i in 0..entry.tables.len() {
        if ingest_journal::table_class(&entry.tables[i].table) != ingest_journal::TableClass::Core {
            continue;
        }
        fence_core_sends(ch, journal, entry, i).await?;
        if entry.tables[i].status == TableStatus::Parked {
            return Err(format!(
                "block {} table {}: terminal evidence saved; Parked decision requires operator reconciliation",
                entry.block_number, entry.tables[i].table
            ).into());
        }
        if core_count(ch, &entry.tables[i].table, entry.block_number).await? > 0 {
            entry.tables[i].status = TableStatus::Done;
            entry.tables[i].last_error = None;
            journal.save(entry)?;
        } else if entry.tables[i].status == TableStatus::Done {
            return Err(format!(
                "block {} table {}: a Done decision has no stored rows; owner retained",
                entry.block_number, entry.tables[i].table
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) fn finish_heal(
    journal: &Journal,
    entry: &Entry,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if entry.kind == EntryKind::Heal {
        journal.finish(entry)?;
    }
    Ok(())
}

/// Called only after the entire intended raw/trace path succeeded. A recorded all-Done prefix alone
/// is insufficient: a crash may have happened before the next table was prepared.
pub(crate) fn complete_heal(
    journal: &Journal,
    entry: &mut Entry,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if entry.kind == EntryKind::Heal {
        if !entry.is_done() || entry.tables.iter().any(|t| !t.sends_fenced()) {
            return Err(format!(
                "block {}: heal still has unfinished or unfenced tables",
                entry.block_number
            )
            .into());
        }
        entry.heal_complete = true;
        journal.save(entry)?;
        journal.finish(entry)?;
    }
    Ok(())
}

/// One core table's rows. The send is durable before the first request byte. An old request must be positively
/// terminal before even a same-generation retry. Only fenced global absence permits a generation advance.
pub(crate) async fn insert_block_rows<T: clickhouse::Row + serde::Serialize>(
    ch: &Client,
    table: &str,
    rows: &[T],
    block_number: u64,
    block_hash: &str,
    journal: &Journal,
    entry: &mut Entry,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if entry.block_number != block_number
        || !entry.block_hash.eq_ignore_ascii_case(block_hash.trim())
    {
        return Err(format!(
            "block {block_number} table {table}: RPC identity differs from its journal entry"
        )
        .into());
    }
    let i = journal.prepare_core(entry, table, rows.len() as u64)?;
    let first_generation = core_generation(entry, table)?;
    let base = ingest_journal::dedup_token(table, block_number, block_hash);
    if entry.tables[i].sends > 0 || entry.tables[i].status != TableStatus::Pending {
        // F2 before F4, as in streamer::complete_entry: fence first (wait, then park); only then may a count decide.
        // A fenced zero count retries under the same generation only if every send was provably never submitted.
        fence_core_sends(ch, journal, entry, i).await?;
        if entry.tables[i].status == TableStatus::Parked {
            return Err(format!(
                "block {block_number} table {table}: terminal evidence saved; Parked decision requires operator reconciliation"
            ).into());
        }
        if core_count(ch, table, block_number).await? > 0 {
            entry.tables[i].status = TableStatus::Done;
            entry.tables[i].last_error = None;
            journal.save(entry)?;
            return Ok(());
        }
        if entry.tables[i].status != TableStatus::Pending {
            return Err(format!(
                "block {block_number} table {table}: {:?} decision has no rows; operator reconciliation required",
                entry.tables[i].status
            ).into());
        }
        if !entry.tables[i].all_rejected() {
            advance_core_generation(journal, entry, i)?;
        }
    }
    for _ in 0..2 {
        let generation = entry.tables[i].generation;
        let token = ingest_journal::generation_token(&base, generation);
        let now_ms = ingest_journal::wall_ms();
        let query_id = ingest_journal::query_id(block_number, table, now_ms);
        record_core_send(
            journal,
            entry,
            table,
            rows.len(),
            generation,
            &query_id,
            now_ms,
        )?;
        let sent = crate::streamer::insert_rows(ch, table, rows, &token, &query_id).await;
        match sent {
            Ok(()) => {
                entry.tables[i].record_client_end(&query_id, ingest_journal::wall_ms())?;
                entry.tables[i].record_terminal(&query_id, false);
                entry.tables[i].last_error = None;
                journal.save(entry)?;
            }
            Err(e) => {
                // The id is already durable. Only a typed connect failure is rejected; a write error may have
                // committed a prefix and stays unfenced.
                entry.tables[i].record_client_end(&query_id, ingest_journal::wall_ms())?;
                if e.not_submitted {
                    entry.tables[i].record_terminal(&query_id, true);
                }
                entry.tables[i].last_error = Some(e.to_string());
                journal.save(entry)?;
                return Err(e.into());
            }
        }
        fence_core_sends(ch, journal, entry, i).await?;
        if core_count(ch, table, block_number).await? > 0 {
            entry.tables[i].status = TableStatus::Done;
            journal.save(entry)?;
            return Ok(());
        }
        error!(
            "TANK-ENGINE INGEST_DEDUP_UNBACKED block={} table={}: all sends under {} are terminal but no rows are stored",
            block_number, table, token
        );
        advance_core_generation(journal, entry, i)?;
    }
    Err(format!(
        "block {block_number} table {table}: no rows after two acknowledged sends starting at generation {first_generation} (next durable generation {})",
        entry.tables[i].generation
    ).into())
}

/// A row of a queue table (world_trace_gaps), synchronous; queue tables keep no deduplication window.
pub(crate) async fn insert_queue_row<T: clickhouse::Row + serde::Serialize>(
    ch: &Client,
    table: &str,
    row: &T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut insert = ch.insert(table)?.with_option("async_insert", "0");
    insert.write(row).await?;
    insert.end().await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    // Tests run on parallel threads and the environment is process-wide: every test that sets a variable holds this
    // lock (env_flags_accept_one_and_true failed when env_flags_reject_garbage set TANK_RAW_HEAL_ENQUEUE meanwhile).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clear_env(keys: &[&str]) {
        for k in keys {
            std::env::remove_var(k);
        }
    }

    pub(crate) struct RecoveryDir(std::path::PathBuf);

    impl RecoveryDir {
        pub(crate) fn new() -> Self {
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("tank-raw-recovery-{}-{n}", std::process::id()));
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }

        fn open(&self) -> Journal {
            Journal::open(&self.0, &self.0.join("writer.lock")).unwrap()
        }

        pub(crate) fn ingest(&self) -> Ingest {
            Ingest {
                journal: self.open(),
                locks: BlockLocks::new(),
                cutover_path: self.0.join("cutover.json"),
                floor: 1,
                windows_since_ms: 1,
                stream_expected: std::sync::atomic::AtomicU64::new(101),
            }
        }
    }

    impl Drop for RecoveryDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[derive(Default)]
    pub(crate) struct RawMockState {
        gaps: std::collections::BTreeMap<u64, SequenceGapRow>,
        presence_checks: Vec<u64>,
        pub(crate) rpc_methods: Vec<String>,
        pub(crate) trace_inserts: usize,
        pub(crate) trace_gap_heights: Vec<u64>,
        pub(crate) trace_gap_resolved: std::collections::BTreeMap<u64, u8>,
        partial_traces: bool,
        pub(crate) query_override:
            Option<Box<dyn FnMut(&str, &[u8]) -> Option<Vec<u8>> + Send>>,
    }

    impl RawMockState {
        fn queue(&mut self, b: u64, source: &str) {
            self.gaps.insert(
                b,
                SequenceGapRow {
                    gap_lo: b,
                    gap_hi: b,
                    detected_at: 1,
                    resolved: 0,
                    source: source.to_string(),
                },
            );
        }

        fn response(&mut self, query: &str, body: &[u8]) -> Vec<u8> {
            if let Some(response) = self.query_override.as_mut().and_then(|reply| reply(query, body)) {
                return response;
            }
            if let Ok(rpc) = serde_json::from_slice::<Value>(body) {
                if let Some(method) = rpc.get("method").and_then(Value::as_str) {
                    self.rpc_methods.push(method.to_string());
                    let result = match method {
                        "eth_getBlockByNumber" => json!({
                            "hash": "0xabc",
                            "timestamp": format!("0x{:x}", 1_760_000_000u64),
                            "transactions": [
                                {"hash": "0x01", "from": "0x11", "to": "0x22"},
                                {"hash": "0x02", "from": "0x11", "to": "0x22"}
                            ]
                        }),
                        "eth_getBlockReceipts" => json!([
                            {"transactionHash": "0x01", "transactionIndex": "0x0", "status": "0x1", "logs": []},
                            {"transactionHash": "0x02", "transactionIndex": "0x1", "status": "0x1", "logs": []}
                        ]),
                        "trace_block" => {
                            let count = if self.partial_traces { 1 } else { 2 };
                            Value::Array(
                                (0..count)
                                    .map(|i| json!({
                                        "transactionHash": format!("0x{:02x}", i + 1),
                                        "transactionPosition": i,
                                        "type": "call",
                                        "action": {"from": "0x11", "to": "0x22", "value": "0x0", "gas": "0x5208", "callType": "call"},
                                        "result": {"gasUsed": "0x5208"},
                                        "traceAddress": []
                                    }))
                                    .collect(),
                            )
                        }
                        _ => panic!("unexpected mock RPC method: {method}"),
                    };
                    return serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": 1, "result": result}))
                        .unwrap();
                }
            }
            if query.starts_with("CREATE TABLE IF NOT EXISTS") {
                return Vec::new();
            }
            if query.contains("SELECT gap_lo, gap_hi") {
                if query.contains("LIMIT 5") {
                    return Vec::new();
                }
                assert!(query.contains("LIMIT 20"), "{query}");
                return self
                    .gaps
                    .values()
                    .filter(|gap| gap.resolved == 0)
                    .take(20)
                    .flat_map(|gap| {
                        gap.gap_lo
                            .to_le_bytes()
                            .into_iter()
                            .chain(gap.gap_hi.to_le_bytes())
                    })
                    .collect();
            }
            if query.contains("SELECT number AS b FROM numbers(") {
                let b: u64 = query
                    .split("numbers(")
                    .nth(1)
                    .unwrap()
                    .split(',')
                    .next()
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                self.presence_checks.push(b);
                return b.to_le_bytes().to_vec();
            }
            if query.starts_with("INSERT INTO world_sequence_gaps") {
                let b = u64::from_le_bytes(body[..8].try_into().unwrap());
                let hi = u64::from_le_bytes(body[8..16].try_into().unwrap());
                assert_eq!(b, hi);
                self.gaps.get_mut(&b).unwrap().resolved = body[24];
                return Vec::new();
            }
            if query.contains(" AS tx_count, ") {
                // Blocks, transactions and logs are already present; only RPC trace coverage is missing.
                return [1u64, 2, 2, 1, 0]
                    .into_iter()
                    .flat_map(u64::to_le_bytes)
                    .collect();
            }
            if query.starts_with("INSERT INTO world_traces") {
                self.trace_inserts += 1;
                return Vec::new();
            }
            if query.starts_with("SELECT tx_index FROM worldchain.world_transactions FINAL") {
                return [0u32, 1].into_iter().flat_map(u32::to_le_bytes).collect();
            }
            if query.contains("SELECT toUInt64(count()) AS n FROM worldchain.world_traces") {
                return (self.trace_inserts as u64).to_le_bytes().to_vec();
            }
            if query.starts_with("INSERT INTO world_trace_gaps") {
                let b = u64::from_le_bytes(body[..8].try_into().unwrap());
                self.trace_gap_heights.push(b);
                let mut offset = 12usize;
                let mut reason_len = 0usize;
                let mut shift = 0usize;
                loop {
                    let byte = body[offset];
                    offset += 1;
                    reason_len |= usize::from(byte & 0x7f) << shift;
                    if byte & 0x80 == 0 {
                        break;
                    }
                    shift += 7;
                }
                self.trace_gap_resolved.insert(b, body[offset + reason_len + 8]);
                return Vec::new();
            }
            panic!("unexpected mock ClickHouse query: {query}");
        }
    }

    pub(crate) struct RawMock {
        pub(crate) ch: Client,
        pub(crate) http: reqwest::Client,
        pub(crate) state: Arc<std::sync::Mutex<RawMockState>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl RawMock {
        pub(crate) async fn new(partial_traces: bool) -> Self {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let state = Arc::new(std::sync::Mutex::new(RawMockState {
                partial_traces,
                ..Default::default()
            }));
            let server_state = state.clone();
            let task = tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut reader = tokio::io::BufReader::new(stream);
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).await.unwrap() > 0);
                    let target = line.split_whitespace().nth(1).unwrap().to_string();
                    let mut length = 0usize;
                    let mut chunked = false;
                    loop {
                        line.clear();
                        assert!(reader.read_line(&mut line).await.unwrap() > 0);
                        if line == "\r\n" {
                            break;
                        }
                        let header = line.to_ascii_lowercase();
                        if let Some(value) = header.strip_prefix("content-length:") {
                            length = value.trim().parse().unwrap();
                        }
                        if header.starts_with("transfer-encoding:") && header.contains("chunked") {
                            chunked = true;
                        }
                        if header.starts_with("expect:") && header.contains("100-continue") {
                            reader
                                .get_mut()
                                .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                                .await
                                .unwrap();
                        }
                    }
                    let mut body = Vec::new();
                    if chunked {
                        loop {
                            line.clear();
                            assert!(reader.read_line(&mut line).await.unwrap() > 0);
                            let size = usize::from_str_radix(
                                line.trim().split(';').next().unwrap(),
                                16,
                            )
                            .unwrap();
                            if size == 0 {
                                loop {
                                    line.clear();
                                    assert!(reader.read_line(&mut line).await.unwrap() > 0);
                                    if line == "\r\n" {
                                        break;
                                    }
                                }
                                break;
                            }
                            let start = body.len();
                            body.resize(start + size, 0);
                            reader.read_exact(&mut body[start..]).await.unwrap();
                            let mut crlf = [0; 2];
                            reader.read_exact(&mut crlf).await.unwrap();
                            assert_eq!(&crlf, b"\r\n");
                        }
                    } else {
                        body.resize(length, 0);
                        reader.read_exact(&mut body).await.unwrap();
                    }
                    let uri = reqwest::Url::parse(&target)
                        .or_else(|_| reqwest::Url::parse(&format!("http://mock{target}")))
                        .unwrap();
                    let query = uri
                        .query_pairs()
                        .find(|(key, _)| key == "query")
                        .map(|(_, value)| value.into_owned())
                        .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
                    let response = server_state.lock().unwrap().response(query.trim(), &body);
                    let mut stream = reader.into_inner();
                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                response.len()
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    stream.write_all(&response).await.unwrap();
                }
            });
            Self {
                ch: Client::default()
                    .with_url(&url)
                    .with_database(CLICKHOUSE_DB)
                    .with_compression(clickhouse::Compression::None),
                // Redirect the hardcoded archive URL to this per-test listener, never the real RPC.
                http: reqwest::Client::builder()
                    .proxy(reqwest::Proxy::all(&url).unwrap())
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap(),
                state,
                task,
            }
        }
    }

    impl Drop for RawMock {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    fn gap_owner(
        journal: &Journal,
        b: u64,
        kind: EntryKind,
        complete_traces: bool,
        parked: bool,
        sent: bool,
    ) -> Entry {
        let mut block = BlockNotification {
            number: b,
            hash: format!("0x{b:064x}"),
            timestamp: 1_760_000_000,
            tx_count: 2,
            txs: (0..2)
                .map(|tx_index| crate::nodestream::Transaction {
                    tx_index,
                    ..Default::default()
                })
                .collect(),
            traces: vec![crate::nodestream::Trace::default()],
            ..Default::default()
        };
        if complete_traces {
            block.traces.push(crate::nodestream::Trace {
                tx_index: 1,
                ..Default::default()
            });
        }
        let mut entry = Entry::new(
            kind,
            b,
            &block.hash,
            block.timestamp,
            100,
            &[
                ("world_transactions", 2),
                ("world_traces", block.traces.len() as u64),
                (ingest_journal::LEDGER_TABLE, 1),
            ],
        );
        if parked {
            entry.tables[0].status = TableStatus::Parked;
        }
        if sent {
            entry.tables[0].record_send("prior-send", 100).unwrap();
            entry.tables[0].record_terminal("prior-send", false);
        }
        journal.create(&mut entry, &block.encode_to_vec()).unwrap();
        entry
    }

    fn trace_retry(journal: &Journal, b: u64, rows: u64) {
        let mut entry = journal.create_heal(b, "0xabc", 1_760_000_000, 100).unwrap();
        // A stop after preparing a trace send leaves recorded work that must resume even with RAW_HEAL_SKIP_TRACES=1.
        // Using that existing rule keeps these RPC regressions independent of process-wide environment settings.
        journal.prepare_core(&mut entry, "world_traces", rows).unwrap();
    }

    #[tokio::test]
    async fn drain_passes_twenty_nonretiring_owners_to_reach_the_stream_trace_gap() {
        let dir = RecoveryDir::new();
        let ingest = dir.ingest();
        let mock = RawMock::new(false).await;
        let mut owners = Vec::new();
        for b in 10..30 {
            let mode = b % 4;
            owners.push(gap_owner(
                &ingest.journal,
                b,
                if mode == 0 { EntryKind::Derive } else { EntryKind::Stream },
                mode == 2,
                mode == 1,
                mode == 3,
            ));
            mock.state.lock().unwrap().queue(b, "heal-failed-retry");
        }
        mock.state.lock().unwrap().queue(100, "stream-missing-traces");
        trace_retry(&ingest.journal, 100, 2);

        // The first tick selects exactly the 20 low owners. The next must reach the higher claimed height.
        for _ in 0..2 {
            tokio::time::timeout(
                Duration::from_secs(5),
                drain_sequence_gaps(&mock.ch, &mock.http, 25, &ingest),
            )
            .await
            .unwrap()
            .unwrap();
        }
        let state = mock.state.lock().unwrap();
        assert!(state.presence_checks.contains(&100), "lower Owned rows starved the trace gap");
        assert!(state.gaps.values().all(|gap| gap.resolved == 1));
        assert_eq!(
            state.rpc_methods,
            ["eth_getBlockByNumber", "eth_getBlockReceipts", "trace_block"]
        );
        assert_eq!(state.trace_inserts, 1);
        assert!(state.trace_gap_heights.is_empty());
        drop(state);
        for owner in owners {
            assert_eq!(
                ingest.journal.entry(owner.block_number).unwrap().unwrap(),
                owner,
                "resolving the queue must not transfer or alter its existing owner"
            );
        }
        assert!(!ingest.journal.owns(100));
    }

    #[tokio::test]
    async fn drain_retains_unsent_stream_gap_through_retired_done_restart_until_finish() {
        let dir = RecoveryDir::new();
        let ingest = dir.ingest();
        let mock = RawMock::new(false).await;
        let mut entry = gap_owner(&ingest.journal, 100, EntryKind::Stream, false, false, false);
        ingest.journal.claim_gap(100, 100, "stream-missing-traces", 100).unwrap();
        mock.state.lock().unwrap().queue(100, "stream-missing-traces");
        drain_sequence_gaps(&mock.ch, &mock.http, 25, &ingest).await.unwrap();
        assert_eq!(mock.state.lock().unwrap().gaps[&100].resolved, 0);
        assert_eq!(ingest.journal.entry(100).unwrap().unwrap(), entry);
        assert!(mock.state.lock().unwrap().rpc_methods.is_empty());

        // Retirement saved Done without ever sending, then stopped before removing the payload and owner.
        for table in &mut entry.tables {
            table.status = TableStatus::Done;
        }
        ingest.journal.save(&mut entry).unwrap();
        drop(ingest);
        let ingest = dir.ingest();
        let retired = ingest.journal.entry(100).unwrap().unwrap();
        assert!(retired.is_done());
        assert!(ingest.journal.entry_payload(&retired).unwrap().is_some());
        drain_sequence_gaps(&mock.ch, &mock.http, 25, &ingest).await.unwrap();
        assert_eq!(mock.state.lock().unwrap().gaps[&100].resolved, 0);
        assert!(mock.state.lock().unwrap().rpc_methods.is_empty());
        assert_eq!(ingest.journal.entry(100).unwrap().unwrap(), retired);

        ingest.journal.finish(&retired).unwrap();
        assert!(!ingest.journal.owns(100));
        trace_retry(&ingest.journal, 100, 2);
        drain_sequence_gaps(&mock.ch, &mock.http, 25, &ingest).await.unwrap();
        let state = mock.state.lock().unwrap();
        assert_eq!(state.gaps[&100].resolved, 1);
        assert_eq!(state.trace_inserts, 1);
        assert!(!ingest.journal.owns(100));
    }

    #[tokio::test]
    async fn partial_trace_block_rpc_queues_trace_gap_without_inserting_partial_traces() {
        let dir = RecoveryDir::new();
        let journal = dir.open();
        let mock = RawMock::new(true).await;
        trace_retry(&journal, 100, 1);
        let err = tokio::time::timeout(
            Duration::from_secs(5),
            heal_block(&mock.ch, &mock.http, 100, &journal, &BlockLocks::new()),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(err.to_string().contains("heal still has unfinished or unfenced tables"), "{err}");
        let state = mock.state.lock().unwrap();
        assert_eq!(
            state.rpc_methods,
            ["eth_getBlockByNumber", "eth_getBlockReceipts", "trace_block"]
        );
        assert_eq!(state.trace_gap_heights, vec![100]);
        assert_eq!(state.trace_inserts, 0, "one of two transaction indices is not covered");
        let retained = journal.entry(100).unwrap().unwrap();
        assert_eq!(retained.kind, EntryKind::Heal);
        assert_eq!(retained.tables[0].status, TableStatus::Pending);
        assert_eq!(retained.tables[0].sends, 0);
        assert!(!retained.heal_complete);
    }

    fn recovery_entry() -> Entry {
        let mut entry = Entry::new(
            ingest_journal::EntryKind::Stream,
            77,
            "0xabc",
            1_760_000_000,
            100,
            &[
                ("world_traces", 8),
                ("world_logs", 4),
                ("api_token_balances", 1),
            ],
        );
        entry.tables[0].generation = 7;
        entry.tables[0].sends = 2;
        entry.tables[0].first_send_ms = Some(100);
        entry.tables[0].query_ids = vec!["old-send".into(), "old-send-2".into()];
        entry.tables[0].terminal_query_ids = entry.tables[0].query_ids.clone();
        entry.tables[1].status = TableStatus::Done;
        entry.tables[2].status = TableStatus::Parked;
        entry
    }

    #[test]
    fn core_recovery_journals_current_generation_and_h1_advance_durably() {
        let dir = RecoveryDir::new();
        let journal = dir.open();
        let mut entry = recovery_entry();
        journal.create(&mut entry, b"lost-payload").unwrap();
        std::fs::remove_file(dir.0.join("77.block")).unwrap();
        assert!(journal.lost_payload(77));
        let other_tables = entry.tables[1..].to_vec();

        let generation = core_generation(&entry, "world_traces").unwrap();
        assert_eq!(generation, 7);
        let i = record_core_send(
            &journal,
            &mut entry,
            "world_traces",
            8,
            generation,
            "heal-g7",
            200,
        )
        .unwrap();
        let saved = journal.entry(77).unwrap().unwrap();
        assert_eq!(saved.tables[0].generation, 7);
        assert_eq!(saved.tables[0].sends, 3);
        assert_eq!(saved.tables[0].first_send_ms, Some(100));
        assert_eq!(
            saved.tables[0].query_ids,
            ["old-send", "old-send-2", "heal-g7"]
        );
        assert_eq!(saved.tables[1..], other_tables);

        // A synchronous ACK fences only its own send, never an older request.
        assert!(advance_core_generation(&journal, &mut entry, i).is_err());
        assert_eq!(journal.entry(77).unwrap().unwrap(), saved);
        entry.tables[i].record_terminal("heal-g7", false);
        journal.save(&mut entry).unwrap();
        // After all terminals are durable, the caller may advance on a global zero count.
        assert_eq!(advance_core_generation(&journal, &mut entry, i).unwrap(), 8);
        let saved = journal.entry(77).unwrap().unwrap();
        assert_eq!(saved.tables[0].generation, 8);
        assert_eq!(saved.tables[0].sends, 0);
        assert_eq!(saved.tables[0].first_send_ms, None);
        assert!(saved.tables[0].query_ids.is_empty());
        assert!(saved.tables[0].terminal_query_ids.is_empty());
        assert!(saved.tables[0].rejected_query_ids.is_empty());
        assert_eq!(saved.tables[0].status, TableStatus::Pending);
        assert_eq!(saved.tables[1..], other_tables);

        record_core_send(&journal, &mut entry, "world_traces", 8, 8, "heal-g8", 201).unwrap();
        let saved = journal.entry(77).unwrap().unwrap();
        assert_eq!(saved.tables[0].generation, 8);
        assert_eq!(saved.tables[0].sends, 1);
        assert_eq!(saved.tables[0].first_send_ms, Some(201));
        assert_eq!(saved.tables[0].query_ids, ["heal-g8"]);
        assert_eq!(saved.tables[1..], other_tables);
    }

    /// A failed terminal-evidence lookup is an error to retry, never a decision: even with the fence wait over, the
    /// table stays Pending and owned with its send history unchanged on disk (it used to park on a transient failure).
    #[tokio::test]
    async fn failed_terminal_lookup_is_retried_never_parked() {
        let dir = RecoveryDir::new();
        let journal = dir.open().with_fence_wait_ms(0);
        let mut entry = recovery_entry();
        journal.create(&mut entry, b"payload").unwrap();
        let id = ingest_journal::query_id(77, "world_traces", 200);
        entry.tables[0].record_send(&id, 200).unwrap();
        journal.save(&mut entry).unwrap();
        let saved = journal.entry(77).unwrap().unwrap();
        // nothing listens on the discard port: the lookup fails before any evidence exists
        let ch = Client::default().with_url("http://127.0.0.1:9");
        let err = fence_core_sends(&ch, &journal, &mut entry, 0)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("terminal evidence lookup failed"),
            "{err}"
        );
        assert_eq!(entry.tables[0].status, TableStatus::Pending);
        assert_eq!(journal.entry(77).unwrap().unwrap(), saved);
    }

    #[test]
    fn core_recovery_refuses_decided_mismatched_or_untracked_sends() {
        let dir = RecoveryDir::new();
        let journal = dir.open();
        let mut entry = recovery_entry();
        journal.create(&mut entry, b"payload").unwrap();

        for status in [TableStatus::Done, TableStatus::Parked, TableStatus::Pending] {
            entry.tables[0].status = status;
            journal.save(&mut entry).unwrap();
            let saved = entry.clone();
            // Pending is also refused when its generation or expected row count differs.
            for (rows, generation) in [(8, 0), (9, 7)] {
                assert!(record_core_send(
                    &journal,
                    &mut entry,
                    "world_traces",
                    rows,
                    generation,
                    "refused",
                    200,
                )
                .is_err());
                assert_eq!(entry, saved);
                assert_eq!(journal.entry(77).unwrap().unwrap(), saved);
            }
            if status != TableStatus::Pending {
                assert!(record_core_send(
                    &journal,
                    &mut entry,
                    "world_traces",
                    8,
                    7,
                    "refused",
                    200,
                )
                .is_err());
                assert_eq!(entry, saved);
                assert_eq!(journal.entry(77).unwrap().unwrap(), saved);
            }
        }

        let saved = entry.clone();
        assert!(core_generation(&entry, "world_blocks").is_err());
        assert!(
            record_core_send(&journal, &mut entry, "world_blocks", 1, 0, "refused", 200,).is_err()
        );
        assert_eq!(journal.entry(77).unwrap().unwrap(), saved);

        entry.tables[0].generation = u32::MAX;
        journal.save(&mut entry).unwrap();
        let saved = entry.clone();
        assert!(advance_core_generation(&journal, &mut entry, 0).is_err());
        assert_eq!(entry, saved);
        assert_eq!(journal.entry(77).unwrap().unwrap(), saved);
    }

    #[test]
    fn truncated_v3_history_never_permits_a_core_replay_or_generation_change() {
        let dir = RecoveryDir::new();
        let journal = dir.open();
        let mut entry = recovery_entry();
        entry.tables[0].query_ids.remove(0);
        entry.tables[0].terminal_query_ids.clear();
        let id = entry.tables[0].query_ids[0].clone();
        entry.tables[0].record_terminal(&id, false);
        journal.create(&mut entry, b"payload").unwrap();
        let saved = entry.clone();
        assert!(!entry.tables[0].sends_fenced());
        assert!(
            record_core_send(&journal, &mut entry, "world_traces", 8, 7, "new-send", 200,).is_err()
        );
        assert!(advance_core_generation(&journal, &mut entry, 0).is_err());
        assert_eq!(entry, saved);
        assert_eq!(journal.entry(77).unwrap().unwrap(), saved);
    }

    #[test]
    fn core_heal_manifest_and_recorded_send_are_durable() {
        let dir = RecoveryDir::new();
        let journal = dir.open();
        let mut entry = journal
            .create_heal(77, "0xabc", 1_760_000_000, 100)
            .unwrap();
        let i = journal.prepare_core(&mut entry, "world_traces", 8).unwrap();
        assert_eq!(core_generation(&entry, "world_traces").unwrap(), 0);
        record_core_send(
            &journal,
            &mut entry,
            "world_traces",
            8,
            0,
            "first-send",
            200,
        )
        .unwrap();
        let saved = journal.entry(77).unwrap().unwrap();
        assert_eq!(saved.kind, EntryKind::Heal);
        assert_eq!(saved.tables[i].query_ids, ["first-send"]);
        assert_eq!(saved.tables[i].sends, 1);
        entry.tables[i].status = TableStatus::Done;
        journal.save(&mut entry).unwrap();
        assert!(
            finish_heal(&journal, &entry).is_err(),
            "presence cannot fence an unknown send"
        );
        assert!(journal.owns(77));
        entry.tables[i].record_terminal("first-send", false);
        journal.save(&mut entry).unwrap();
        complete_heal(&journal, &mut entry).unwrap();
        assert!(!journal.owns(77));
    }

    #[test]
    fn completed_zero_log_heal_survives_restart_and_finishes_without_archive_rpc() {
        let dir = RecoveryDir::new();
        {
            let journal = dir.open();
            let mut entry = journal
                .create_heal(88, "0xabc", 1_760_000_000, 100)
                .unwrap();
            for (table, rows, id) in [
                ("world_transactions", 2, "transactions"),
                ("world_blocks", 1, "block"),
            ] {
                let i = journal.prepare_core(&mut entry, table, rows).unwrap();
                entry.tables[i].record_send(id, 100).unwrap();
                entry.tables[i].record_terminal(id, false);
                entry.tables[i].status = TableStatus::Done;
            }
            // Simulate a stop after the complete marker was fsynced, before owner removal.
            entry.heal_complete = true;
            journal.save(&mut entry).unwrap();
        }
        let journal = dir.open();
        let entry = journal.entry(88).unwrap().unwrap();
        assert!(entry.heal_complete);
        assert!(!entry.tables.iter().any(|t| t.table == "world_logs"));
        assert!(entry.is_done());
        finish_heal(&journal, &entry).unwrap(); // No RPC client or rebuilt payload is needed.
        assert!(!journal.owns(88));
    }

    #[test]
    fn incomplete_all_done_heal_prefix_is_not_released_after_restart() {
        let dir = RecoveryDir::new();
        {
            let journal = dir.open();
            let mut entry = journal
                .create_heal(89, "0xabc", 1_760_000_000, 100)
                .unwrap();
            let i = journal
                .prepare_core(&mut entry, "world_transactions", 2)
                .unwrap();
            entry.tables[i].record_send("prefix", 100).unwrap();
            entry.tables[i].record_terminal("prefix", false);
            entry.tables[i].status = TableStatus::Done;
            journal.save(&mut entry).unwrap();
        }
        let journal = dir.open();
        let entry = journal.entry(89).unwrap().unwrap();
        assert!(entry.is_done());
        assert!(entry.tables.iter().all(|t| t.sends_fenced()));
        assert!(!entry.heal_complete);
        assert!(finish_heal(&journal, &entry).is_err());
        assert!(
            journal.owns(89),
            "logs and the remaining RPC path have not yet been prepared"
        );
    }

    #[test]
    fn env_flags_default_off() {
        let _env = env_lock();
        clear_env(&[
            "TANK_RAW_HEAL_ENQUEUE",
            "RAW_HEAL_SKIP_TRACES",
            "TANK_DISABLE_SEQUENCE_DRAIN",
        ]);
        assert!(!env_cli_raw_heal_enqueue());
        assert!(!env_raw_heal_skip_traces());
        assert!(!env_disable_sequence_drain());
    }

    #[test]
    fn env_flags_accept_one_and_true() {
        let _env = env_lock();
        clear_env(&[
            "TANK_RAW_HEAL_ENQUEUE",
            "RAW_HEAL_SKIP_TRACES",
            "TANK_DISABLE_SEQUENCE_DRAIN",
        ]);
        std::env::set_var("TANK_RAW_HEAL_ENQUEUE", "1");
        std::env::set_var("RAW_HEAL_SKIP_TRACES", "true");
        std::env::set_var("TANK_DISABLE_SEQUENCE_DRAIN", "YES");
        assert!(env_cli_raw_heal_enqueue());
        assert!(env_raw_heal_skip_traces());
        assert!(env_disable_sequence_drain());
        clear_env(&[
            "TANK_RAW_HEAL_ENQUEUE",
            "RAW_HEAL_SKIP_TRACES",
            "TANK_DISABLE_SEQUENCE_DRAIN",
        ]);
    }

    #[test]
    fn env_flags_reject_garbage() {
        let _env = env_lock();
        clear_env(&["TANK_RAW_HEAL_ENQUEUE"]);
        std::env::set_var("TANK_RAW_HEAL_ENQUEUE", "maybe");
        assert!(!env_cli_raw_heal_enqueue());
        clear_env(&["TANK_RAW_HEAL_ENQUEUE"]);
    }

    #[test]
    fn refill_present_flag_is_opt_in() {
        let _env = env_lock();
        clear_env(&["RAW_HEAL_REFILL_PRESENT"]);
        assert!(!env_raw_heal_refill_present());
        std::env::set_var("RAW_HEAL_REFILL_PRESENT", "1");
        assert!(env_raw_heal_refill_present());
        std::env::set_var("RAW_HEAL_REFILL_PRESENT", "maybe");
        assert!(!env_raw_heal_refill_present());
        clear_env(&["RAW_HEAL_REFILL_PRESENT"]);
    }

    #[test]
    fn stored_rows_present_needs_the_block_row_and_its_transactions() {
        let healthy = StoredRows {
            blocks: 1,
            tx_count: 3,
            txs: 3,
            logs: 9,
            traces: 7,
        };
        assert!(healthy.present());
        assert!(!StoredRows {
            blocks: 0,
            tx_count: 0,
            ..healthy.clone()
        }
        .present()); // no block row
        assert!(!StoredRows {
            txs: 0,
            ..healthy.clone()
        }
        .present()); // bare block row
        assert!(StoredRows {
            tx_count: 0,
            txs: 0,
            ..healthy.clone()
        }
        .present()); // a block with no transactions
        assert!(StoredRows {
            logs: 0,
            traces: 0,
            ..healthy.clone()
        }
        .present()); // lost logs: only REFILL_PRESENT
    }

    #[test]
    fn stored_rows_complete_needs_every_core_table() {
        let all = StoredRows {
            blocks: 1,
            tx_count: 3,
            txs: 3,
            logs: 9,
            traces: 7,
        };
        assert!(all.complete(false));
        assert!(!StoredRows {
            traces: 0,
            ..all.clone()
        }
        .complete(false));
        assert!(StoredRows {
            traces: 0,
            ..all.clone()
        }
        .complete(true));
        assert!(!StoredRows {
            blocks: 0,
            ..all.clone()
        }
        .complete(false)); // rows written, block row lost
        assert!(!StoredRows {
            txs: 0,
            ..all.clone()
        }
        .complete(false)); // bare block row (heal before 2026-10-13)
        assert!(!StoredRows {
            logs: 0,
            ..all.clone()
        }
        .complete(true)); // logs lost after the transactions
    }

    /// Review finding 4: the drain must never mark a row resolved while one of its heights is neither healed nor
    /// queued again; rows above MAX_GAP_CHUNK heights are never resolved automatically.
    #[test]
    fn drain_resolves_a_row_only_when_every_missing_height_is_handled() {
        let ok = |b: u64| (b, Some(HealOutcome::Healed));
        let bad = |b: u64| (b, None);
        // everything healed: resolved, nothing queued again
        assert_eq!(
            decide_row(10, 19, 2, &[ok(11), ok(15)]),
            RowDecision::Resolve { requeue: vec![] }
        );
        // nothing missing (all present): resolved
        assert_eq!(
            decide_row(10, 19, 0, &[]),
            RowDecision::Resolve { requeue: vec![] }
        );
        // budget spent before every missing height was tried: kept
        assert_eq!(decide_row(10, 19, 3, &[ok(11), ok(12)]), RowDecision::Keep);
        assert_eq!(decide_row(10, 19, 1, &[]), RowDecision::Keep);
        // a failed height of a wider row is queued again on its own before the row is resolved
        assert_eq!(
            decide_row(10, 19, 2, &[ok(11), bad(12)]),
            RowDecision::Resolve { requeue: vec![12] }
        );
        // a one-block row that failed stays queued
        assert_eq!(decide_row(7, 7, 1, &[bad(7)]), RowDecision::Keep);
        assert_eq!(
            decide_row(7, 7, 1, &[ok(7)]),
            RowDecision::Resolve { requeue: vec![] }
        );
        // oversized rows (the old drain resolved them unhealed) and inverted rows are never resolved
        assert_eq!(
            decide_row(1, MAX_GAP_CHUNK + 1, 0, &[]),
            RowDecision::Keep,
            "10,001 heights"
        );
        assert_eq!(
            decide_row(1, 33_544_342, 0, &[]),
            RowDecision::Keep,
            "the 2026 bogus [1, tip] row"
        );
        assert_eq!(decide_row(5, 4, 0, &[]), RowDecision::Keep);
        assert_eq!(
            decide_row(1, MAX_GAP_CHUNK, 0, &[]),
            RowDecision::Resolve { requeue: vec![] },
            "10,000 heights"
        );
    }

    #[test]
    fn journal_ownership_cannot_resolve_a_gap_before_unsent_retirement_finishes() {
        let owned = (10, Some(HealOutcome::RetiringStream));
        // A late resolution decided before Stream retirement must not erase its only recovery path.
        assert_eq!(decide_row(10, 10, 1, &[owned]), RowDecision::Keep);
        assert_eq!(
            decide_row(10, 11, 2, &[owned, (11, Some(HealOutcome::Healed))]),
            RowDecision::Resolve { requeue: vec![10] }
        );
        // After owner removal the raw healer may complete the height, or presence may find it already stored.
        assert_eq!(
            decide_row(10, 10, 1, &[(10, Some(HealOutcome::Healed))]),
            RowDecision::Resolve { requeue: vec![] }
        );
        assert_eq!(decide_row(10, 10, 0, &[]), RowDecision::Resolve { requeue: vec![] });
        assert_eq!(
            decide_row(10, 10, 1, &[(10, Some(HealOutcome::Owned))]),
            RowDecision::Resolve { requeue: vec![] },
            "ordinary journal ownership resolves its row instead of pinning the drain prefix"
        );
    }
}
