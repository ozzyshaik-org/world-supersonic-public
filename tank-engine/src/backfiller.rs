use clickhouse::Client;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::ingest_journal::{Ingest, LEDGER_TABLE};
use crate::mv_matcher::match_and_extract_views;
use crate::nodestream::{BlockNotification, Log, Trace, Transaction};
use crate::streamer::{self, ClickHouseTarget, Completion};
use crate::types::*;

const CLICKHOUSE_URL: &str = "http://127.0.0.1:8123";
const CLICKHOUSE_DB: &str = "worldchain";
const INSERT_CHUNK_SIZE: usize = 50_000;

pub async fn run_historical_backfill(
    start_block: u64,
    end_block: u64,
    chunk_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "TANK-ENGINE Backfiller initialized | Range: {} -> {} | Chunk Size: {}",
        start_block, end_block, chunk_size
    );

    let client = Client::default()
        .with_url(CLICKHOUSE_URL)
        .with_database(CLICKHOUSE_DB);

    let mut current_start = start_block;

    while current_start <= end_block {
        let current_end = (current_start + chunk_size - 1).min(end_block);
        let mut success = false;

        for attempt in 1..=3 {
            let start_time = Instant::now();
            info!(
                "TANK-ENGINE Backfiller processing tranche blocks {} -> {} (Attempt {}/3)...",
                current_start, current_end, attempt
            );

            let query_str = format!(
                "SELECT chain_id, block_number, block_hash, tx_hash, tx_index, log_index, address, data, topics, ingested_at, block_timestamp, is_canonical, healed_by FROM worldchain.world_logs WHERE block_number BETWEEN {} AND {} ORDER BY block_number ASC SETTINGS max_execution_time = 120",
                current_start, current_end
            );

            let mut cursor = match client.query(&query_str).fetch::<LogRow>() {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "Backfill query init error on range {}..{} (attempt {}): {}",
                        current_start, current_end, attempt, e
                    );
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let mut transfers_batch = Vec::new();
            let mut swaps_batch = Vec::new();
            let mut universal_events_batch = Vec::new();
            let mut dex_swap_details_batch = Vec::new();
            let mut gas_sponsorships_batch = Vec::new();
            let mut humanity_feeds_batch = Vec::new();
            let mut nft_transfers_batch = Vec::new();
            let mut oracle_prices_batch = Vec::new();
            let mut erc1155_transfers_batch = Vec::new();
            let mut v3_prices_batch = Vec::new();
            let mut whale_alerts_batch = Vec::new();
            let mut lending_liquidations_batch = Vec::new();
            let mut token_balances_batch = Vec::new();
            let mut latest_oracle_prices_batch = Vec::new();
            let mut uniswap_mints_batch = Vec::new();
            let mut morpho_activity_batch = Vec::new();

            let mut logs_processed = 0u64;
            let mut stream_failed = false;

            loop {
                match cursor.next().await {
                    Ok(Some(row)) => {
                        logs_processed += 1;
                        let log_node = Log {
                            address: String::from_utf8_lossy(&row.address.0).to_string(),
                            topics: row
                                .topics
                                .iter()
                                .map(|t| String::from_utf8_lossy(&t.0).to_string())
                                .collect(),
                            data: row.data.clone(),
                            tx_hash: String::from_utf8_lossy(&row.tx_hash.0).to_string(),
                            log_index: row.log_index,
                            tx_index: row.tx_index,
                        };

                        let mut extracted = match_and_extract_views(
                            &log_node,
                            row.block_number,
                            row.block_timestamp,
                            row.ingested_at,
                            (row.ingested_at / 1000) as u32,
                        );

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
                    Ok(None) => break,
                    Err(e) => {
                        warn!(
                            "Stream error on range {}..{} (attempt {}): {}",
                            current_start, current_end, attempt, e
                        );
                        stream_failed = true;
                        break;
                    }
                }
            }

            if stream_failed {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }

            // Flush derived batches in 50,000-row sub-chunks to guarantee sub-second writes
            let mut flush_err = false;
            if !transfers_batch.is_empty() {
                if let Err(e) =
                    insert_batch(&client, "world_token_transfers", &transfers_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !swaps_batch.is_empty() && !flush_err {
                if let Err(e) = insert_batch(&client, "api_live_dex_swaps", &swaps_batch).await {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !universal_events_batch.is_empty() && !flush_err {
                if let Err(e) = insert_batch(
                    &client,
                    "api_universal_defi_events",
                    &universal_events_batch,
                )
                .await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !dex_swap_details_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_dex_swaps", &dex_swap_details_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !gas_sponsorships_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_gas_sponsorship", &gas_sponsorships_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !humanity_feeds_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_humanity_feed", &humanity_feeds_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !nft_transfers_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_nft_transfers", &nft_transfers_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !oracle_prices_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_oracle_prices", &oracle_prices_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !erc1155_transfers_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "world_erc1155_transfers", &erc1155_transfers_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !v3_prices_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_uniswap_v3_prices", &v3_prices_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !whale_alerts_batch.is_empty() && !flush_err {
                if let Err(e) = insert_batch(&client, "api_whale_alerts", &whale_alerts_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !lending_liquidations_batch.is_empty() && !flush_err {
                if let Err(e) = insert_batch(
                    &client,
                    "api_lending_liquidations",
                    &lending_liquidations_batch,
                )
                .await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !token_balances_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_token_balances", &token_balances_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !latest_oracle_prices_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "latest_oracle_prices", &latest_oracle_prices_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !uniswap_mints_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_uniswap_mints", &uniswap_mints_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }
            if !morpho_activity_batch.is_empty() && !flush_err {
                if let Err(e) =
                    insert_batch(&client, "api_morpho_activity", &morpho_activity_batch).await
                {
                    warn!("Insert err: {}", e);
                    flush_err = true;
                }
            }

            if flush_err {
                return Err(format!(
                    "TANK-ENGINE backfill tranche {} -> {} failed during derived inserts; not advancing or replaying an ambiguous acknowledgement; exclude overlapping writers and reconcile already-written/uncertain derived tables before rerun",
                    current_start, current_end
                ).into());
            }

            let elapsed = start_time.elapsed().as_secs_f64();
            info!(
                "TANK-ENGINE Backfiller completed tranche {} -> {} | Logs: {} | Elapsed: {:.2}s",
                current_start, current_end, logs_processed, elapsed
            );

            success = true;
            break;
        }

        if !success {
            return Err(format!(
                "TANK-ENGINE backfill tranche {} -> {} failed after 3 read attempts; not advancing",
                current_start, current_end
            )
            .into());
        }

        current_start = current_end + 1;
    }

    info!("TANK-ENGINE Backfill operation completed successfully!");
    Ok(())
}

async fn insert_batch<T: clickhouse::Row + serde::Serialize>(
    client: &Client,
    table: &str,
    rows: &[T],
) -> Result<(), Box<dyn std::error::Error>> {
    if rows.is_empty() {
        return Ok(());
    }

    for sub_chunk in rows.chunks(INSERT_CHUNK_SIZE) {
        // C1 v2: the engine tables keep a deduplication window, and on 26.8 a token-less insert into such a table is
        // deduplicated by its data hash (out/DEDUP-PROOF.md H2): an identical chunk would be dropped. The historical
        // backfill sends no token and keeps its behaviour from before the windows.
        let mut insert = client
            .insert(table)?
            .with_option("deduplicate_insert", "disable");
        for row in sub_chunk {
            insert.write(row).await?;
        }
        insert.end().await?;
    }

    Ok(())
}

// C1 v2 gap derivation: blocks the raw heal filled (stream gaps, the cutover gap, blocks whose journal payload was
// lost) get their derived tables the way the streamer writes them: the block is rebuilt from its stored core rows,
// saved in the ingest journal as a derive entry, and every derived table plus the ledger row is written under the
// block's deduplication tokens (streamer::derive_block). Only blocks at or above the C1 v2 floor: below it the old
// engine and the repair packs own derivation, and a block they derived partly can't be told from an underived one.

/// Heights scanned per tick, from the lowest one that may lack a ledger row.
const DERIVE_SCAN: u64 = 1_000_000;
/// Blocks derived per tick (the raw heal fills 25 per tick).
const DERIVE_PER_TICK: u64 = 50;

pub struct GapDeriver {
    /// Every height in [floor, low) has a ledger row.
    low: u64,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Height {
    b: u64,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Count {
    n: u64,
}

impl GapDeriver {
    pub fn new(floor: u64) -> GapDeriver {
        GapDeriver { low: floor }
    }

    /// Derives up to DERIVE_PER_TICK unowned ready blocks: passed by the stream, without a ledger row, with their
    /// core rows stored. Owned ready blocks do not consume the budget or hide later gaps. A lost payload is rebuilt
    /// into its existing entry under the same lock.
    pub async fn tick(
        &mut self,
        ch: &Client,
        target: &ClickHouseTarget,
        ingest: &Ingest,
    ) -> Result<(), String> {
        let passed = passed_tip(ch, ingest).await?;
        if passed < self.low {
            return Ok(());
        }
        let lo = self.low;
        let hi = passed.min(lo.saturating_add(DERIVE_SCAN - 1));
        let first: Vec<Height> = ch
            .query(&format!(
                "SELECT number AS b FROM numbers({lo}, {n}) WHERE number NOT IN \
                 (SELECT block_number FROM worldchain.{LEDGER_TABLE} WHERE block_number BETWEEN {lo} AND {hi}) \
                 ORDER BY b LIMIT 1",
                n = hi - lo + 1
            ))
            .fetch_all()
            .await
            .map_err(|e| format!("ledger scan [{lo}, {hi}] failed: {e}"))?;
        let Some(first) = first.first().map(|h| h.b) else {
            self.low = hi + 1;
            return Ok(());
        };
        self.low = first;
        if hi == lo.saturating_add(DERIVE_SCAN - 1) && passed > hi {
            warn!(
                "TANK-ENGINE INGEST_DERIVE_BEHIND lowest height without a ledger row is {} ({} heights below the stream): only [{}, {}] is scanned",
                first,
                passed - first,
                lo,
                hi
            );
        }
        let owned = ingest
            .journal
            .owned_between(first, hi)
            .map_err(|e| format!("journal ownership scan [{first}, {hi}] failed: {e}"))?;
        let ready_limit = DERIVE_PER_TICK.saturating_add(owned.len() as u64);
        let ready: Vec<Height> = ch
            .query(&format!(
                "SELECT block_number AS b FROM ( \
                   SELECT block_number, max(transactions_count) AS tc FROM worldchain.world_blocks \
                   WHERE chain_id = 480 AND block_number BETWEEN {first} AND {hi} GROUP BY block_number) \
                 WHERE block_number NOT IN (SELECT block_number FROM worldchain.{LEDGER_TABLE} WHERE block_number BETWEEN {first} AND {hi}) \
                   AND (tc = 0 OR (block_number IN (SELECT block_number FROM worldchain.world_transactions WHERE chain_id = 480 AND block_number BETWEEN {first} AND {hi}) \
                   AND block_number IN (SELECT block_number FROM worldchain.world_traces WHERE chain_id = 480 AND block_number BETWEEN {first} AND {hi}))) \
                 ORDER BY b LIMIT {ready_limit}"
            ))
            .fetch_all()
            .await
            .map_err(|e| format!("ready scan [{first}, {hi}] failed: {e}"))?;
        let mut derived = 0u64;
        let mut attempted = 0u64;
        for Height { b } in ready {
            let _owner = ingest.locks.lock(b).await;
            if ingest.journal.owns(b) {
                if ingest.journal.lost_payload(b) {
                    streamer::resume_owned(target, ingest, b).await?;
                }
                continue;
            }
            if attempted == DERIVE_PER_TICK {
                break;
            }
            attempted += 1;
            let held: Count = ch
                .query(&format!("SELECT toUInt64(count()) AS n FROM worldchain.{LEDGER_TABLE} WHERE block_number = {b}"))
                .fetch_one()
                .await
                .map_err(|e| format!("block {b}: ledger check failed: {e}"))?;
            if held.n > 0 {
                continue;
            }
            let Some((block, now_ms)) = load_stored_block(ch, b).await? else {
                continue;
            };
            match streamer::derive_block(target, ingest, &block, now_ms).await {
                Ok(Completion::Finished) => derived += 1,
                Ok(Completion::Parked) => {}
                Err(e) => {
                    // the derive entry stays in the journal; resumed on the next tick
                    warn!("TANK-ENGINE gap derive block={} not finished yet: {}", b, e);
                    break;
                }
            }
        }
        if derived > 0 {
            info!("TANK-ENGINE gap derive: {} block(s) derived; lowest height without a ledger row {}", derived, self.low);
        }
        Ok(())
    }
}

/// The highest height the stream has passed: below its subscribed position (published at connect,
/// before the next block), and below the newest finished stream ledger row. A restart with no
/// newer stream commit still derives a heal the previous connection already walked past.
async fn passed_tip(ch: &Client, ingest: &Ingest) -> Result<u64, String> {
    #[derive(Debug, clickhouse::Row, serde::Deserialize)]
    struct Tip {
        b: u64,
    }
    let in_memory = ingest
        .stream_expected
        .load(Ordering::SeqCst)
        .saturating_sub(1);
    let stored: Tip = ch
        .query(&format!(
            "SELECT toUInt64(ifNull(max(block_number), 0)) AS b FROM worldchain.{LEDGER_TABLE} WHERE source = 'stream' AND block_number >= {}",
            ingest.floor
        ))
        .fetch_one()
        .await
        .map_err(|e| format!("ledger tip query failed: {e}"))?;
    Ok(in_memory.max(stored.b.saturating_sub(1)))
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct StoredBlock {
    hash: String,
    parent: String,
    ts: u64,
    miner: String,
    gas_limit: u64,
    gas_used: u64,
    base_fee: u64,
    tx_count: u32,
    size_bytes: u32,
    ingested_ms: u64,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct StoredTx {
    tx_index: u32,
    hash: String,
    from_a: String,
    to_a: String,
    value: String,
    gas_price: u64,
    max_fee: u64,
    max_priority: u64,
    gas_limit: u64,
    gas_used: u64,
    nonce: u64,
    tx_type: u8,
    input: String,
    status: u8,
    l1_fee: String,
    l1_gas_price: u64,
    mint: String,
    is_system: u8,
    ingested_ms: u64,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct StoredLog {
    tx_hash: String,
    tx_index: u32,
    log_index: u32,
    address: String,
    topics: Vec<String>,
    data: String,
    ingested_ms: u64,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct StoredTrace {
    tx_index: u32,
    tx_hash: String,
    from_a: String,
    to_a: String,
    value: String,
    input: String,
    output: String,
    gas: u64,
    gas_used: u64,
    trace_type: String,
    call_type: String,
    error: String,
    trace_index: u32,
}

/// A stored fixed-width string as the writers built it ("0x…", lowercase), without NUL padding.
fn s(col: &str) -> String {
    format!("replaceAll(lower(toString({col})), '\\0', '')")
}

/// The block rebuilt from its stored core rows in the ExEx shape, and the ingested_at (ms) for a new derive entry:
/// that of its stored logs (else transactions, else block row). Payload recovery instead keeps its existing entry's
/// ingested_at and every table decision. None (logged) when the block has no block row, incomplete transactions/traces,
/// or more than one hash (a reorg is never derived automatically).
pub(crate) async fn load_stored_block(
    ch: &Client,
    b: u64,
) -> Result<Option<(BlockNotification, u64)>, String> {
    let q = |what: &str, e: clickhouse::error::Error| {
        format!("block {b}: stored {what} unreadable: {e}")
    };
    let blocks: Vec<StoredBlock> = ch
        .query(&format!(
            "SELECT {}, {}, toUInt64(toUnixTimestamp(timestamp)), {}, gas_limit, gas_used, base_fee_per_gas, \
             transactions_count, size_bytes, toUInt64(toUnixTimestamp64Milli(ingested_at)) \
             FROM worldchain.world_blocks WHERE chain_id = 480 AND block_number = {b} \
             ORDER BY ingested_at LIMIT 1 BY block_hash",
            s("block_hash"),
            s("parent_hash"),
            s("miner")
        ))
        .fetch_all()
        .await
        .map_err(|e| q("block row", e))?;
    let block_row = match blocks.as_slice() {
        [one] => one,
        [] => return Ok(None),
        many => {
            error!(
                "TANK-ENGINE INGEST_DERIVE_REFUSED block={} has {} block hashes ({}): a reorg is never derived automatically",
                b,
                many.len(),
                many.iter().map(|r| r.hash.as_str()).collect::<Vec<_>>().join(",")
            );
            return Ok(None);
        }
    };
    let txs: Vec<StoredTx> = ch
        .query(&format!(
            "SELECT tx_index, {}, {}, {}, toString(value_wei), gas_price_wei, max_fee_per_gas_wei, max_priority_fee_wei, \
             gas_limit, gas_used, nonce, tx_type, input_data, status, toString(l1_fee_wei), l1_gas_price_wei, \
             toString(mint_wei), is_system_tx, toUInt64(toUnixTimestamp64Milli(ingested_at)) \
             FROM worldchain.world_transactions WHERE chain_id = 480 AND block_number = {b} \
             ORDER BY tx_index, ingested_at LIMIT 1 BY tx_index",
            s("tx_hash"),
            s("from_address"),
            s("to_address")
        ))
        .fetch_all()
        .await
        .map_err(|e| q("transactions", e))?;
    let logs: Vec<StoredLog> = ch
        .query(&format!(
            "SELECT {}, tx_index, log_index, {}, arrayMap(t -> replaceAll(lower(toString(t)), '\\0', ''), topics), data, \
             toUInt64(toUnixTimestamp64Milli(ingested_at)) \
             FROM worldchain.world_logs WHERE chain_id = 480 AND block_number = {b} \
             ORDER BY tx_index, log_index, ingested_at LIMIT 1 BY tx_index, log_index",
            s("tx_hash"),
            s("address")
        ))
        .fetch_all()
        .await
        .map_err(|e| q("logs", e))?;
    let traces: Vec<StoredTrace> = ch
        .query(&format!(
            "SELECT tx_index, {}, {}, {}, toString(value_wei), input_data, output_data, gas_limit, gas_used, \
             toString(trace_type), toString(call_type), toString(error), trace_index \
             FROM worldchain.world_traces WHERE chain_id = 480 AND block_number = {b} \
             ORDER BY tx_index, trace_index, ingested_at LIMIT 1 BY tx_index, trace_index",
            s("tx_hash"),
            s("from_address"),
            s("to_address")
        ))
        .fetch_all()
        .await
        .map_err(|e| q("traces", e))?;
    if txs.len() != block_row.tx_count as usize || (block_row.tx_count > 0 && traces.is_empty()) {
        warn!(
            "TANK-ENGINE INGEST_PAYLOAD_WAIT_CORE block={}: stored txs={} expected={} traces={}",
            b,
            txs.len(),
            block_row.tx_count,
            traces.len()
        );
        return Ok(None);
    }
    let now_ms = logs
        .iter()
        .map(|l| l.ingested_ms)
        .min()
        .or_else(|| txs.iter().map(|t| t.ingested_ms).min())
        .unwrap_or(block_row.ingested_ms);
    let block = rebuild_block(b, block_row, txs, logs, traces);
    Ok(Some((block, now_ms)))
}

/// The zero address the writers store for a contract creation (no `to`): the streamer derives no interaction for it.
fn creation_to(to: &str) -> bool {
    to.is_empty() || to == "0x0000000000000000000000000000000000000000"
}

fn rebuild_block(
    b: u64,
    row: &StoredBlock,
    txs: Vec<StoredTx>,
    logs: Vec<StoredLog>,
    traces: Vec<StoredTrace>,
) -> BlockNotification {
    BlockNotification {
        number: b,
        hash: row.hash.clone(),
        parent_hash: row.parent.clone(),
        timestamp: row.ts,
        miner: row.miner.clone(),
        gas_limit: row.gas_limit,
        gas_used: row.gas_used,
        base_fee: row.base_fee,
        tx_count: row.tx_count,
        size_bytes: row.size_bytes,
        txs: txs
            .into_iter()
            .map(|t| Transaction {
                tx_index: t.tx_index,
                to: if creation_to(&t.to_a) {
                    String::new()
                } else {
                    t.to_a
                },
                hash: t.hash,
                from: t.from_a,
                value: t.value,
                gas_price: t.gas_price,
                max_fee_per_gas: t.max_fee,
                max_priority_fee: t.max_priority,
                gas_limit: t.gas_limit,
                gas_used: t.gas_used,
                nonce: t.nonce,
                tx_type: u32::from(t.tx_type),
                input: t.input,
                status: u32::from(t.status),
                l1_fee: t.l1_fee,
                l1_gas_price: t.l1_gas_price,
                mint_wei: t.mint,
                is_system_tx: t.is_system != 0,
            })
            .collect(),
        traces: traces
            .into_iter()
            .map(|t| Trace {
                tx_index: t.tx_index,
                tx_hash: t.tx_hash,
                from_address: t.from_a,
                to_address: t.to_a,
                value_wei: t.value,
                input_data: t.input,
                output_data: t.output,
                gas: t.gas,
                gas_used: t.gas_used,
                trace_type: t.trace_type,
                call_type: t.call_type,
                revert_reason: t.error,
                trace_index: t.trace_index,
            })
            .collect(),
        logs: logs
            .into_iter()
            .map(|l| Log {
                tx_hash: l.tx_hash,
                tx_index: l.tx_index,
                log_index: l.log_index,
                address: l.address,
                topics: l.topics,
                data: l.data,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest_journal::EntryKind;
    use crate::mv_matcher::{TOPIC_TRANSFER, TOPIC_V2_SWAP};

    fn word(n: u64) -> String {
        format!("0x{n:064x}")
    }

    fn addr(n: u64) -> String {
        format!("0x{n:040x}")
    }

    /// A stored block rebuilt for derivation writes the derived tables the streamer writes for the same block, with
    /// the same rows (the ExEx sends lowercase hex; stored strings are lowercase), and no creation interaction.
    #[test]
    fn rebuilt_block_derives_the_streamers_rows() {
        let row = StoredBlock {
            hash: word(9),
            parent: word(8),
            ts: 1_760_000_000,
            miner: addr(0x11),
            gas_limit: 30_000_000,
            gas_used: 100_000,
            base_fee: 7,
            tx_count: 2,
            size_bytes: 900,
            ingested_ms: 1,
        };
        let tx = |i: u32, to: &str, input: &str| StoredTx {
            tx_index: i,
            hash: word(100 + u64::from(i)),
            from_a: addr(0xbb),
            to_a: to.to_string(),
            value: "1000000000000000000000".into(),
            gas_price: 5,
            max_fee: 6,
            max_priority: 1,
            gas_limit: 50_000,
            gas_used: 21_000,
            nonce: 3,
            tx_type: 2,
            input: input.to_string(),
            status: 1,
            l1_fee: "7".into(),
            l1_gas_price: 2,
            mint: "0".into(),
            is_system: 0,
            ingested_ms: 5,
        };
        let txs = vec![
            tx(0, &addr(0xaa), "0xa9059cbb0000"),
            tx(
                1,
                "0x0000000000000000000000000000000000000000",
                "0x6080604052",
            ), // contract creation
        ];
        let log = |i: u32, topics: Vec<String>| StoredLog {
            tx_hash: word(100),
            tx_index: 0,
            log_index: i,
            address: addr(0xaa),
            topics,
            data: word(3),
            ingested_ms: 4,
        };
        let logs = vec![
            log(0, vec![TOPIC_TRANSFER.into(), word(0xbb), word(0xcc)]),
            log(1, vec![TOPIC_V2_SWAP.into()]),
            log(2, vec![TOPIC_V2_SWAP.into()]),
        ];
        let traces = vec![StoredTrace {
            tx_index: 0,
            tx_hash: word(100),
            from_a: addr(0xbb),
            to_a: addr(0xcc),
            value: "5".into(),
            input: "0x".into(),
            output: "0x".into(),
            gas: 1,
            gas_used: 1,
            trace_type: "call".into(),
            call_type: "call".into(),
            error: String::new(),
            trace_index: 0,
        }];
        let rebuilt = rebuild_block(9, &row, txs, logs, traces);
        assert_eq!(
            rebuilt.txs[1].to, "",
            "the stored zero address of a creation is no `to`"
        );
        let tables: Vec<&str> = streamer::build_batches(&rebuilt, 4)
            .tables(EntryKind::Derive)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        for t in [
            "world_token_transfers",
            "api_live_dex_swaps",
            "world_contract_interactions",
            "api_gas_by_function",
            "api_internal_transfers",
            "world_mev_hourly_stats",
            "api_token_balances",
            LEDGER_TABLE,
        ] {
            assert!(tables.contains(&t), "{t} in {tables:?}");
        }
        let rows: Vec<(&str, u64)> = streamer::build_batches(&rebuilt, 4).tables(EntryKind::Derive);
        let n = |t: &str| {
            rows.iter()
                .find(|(x, _)| *x == t)
                .map(|(_, n)| *n)
                .unwrap_or(0)
        };
        assert_eq!(
            n("world_contract_interactions"),
            1,
            "one call, the creation left out"
        );
        assert_eq!(n("api_token_balances"), 2);
        assert_eq!(n("world_mev_hourly_stats"), 1);
        assert!(!tables.contains(&"world_transactions") && !tables.contains(&"world_blocks"));
    }
}
