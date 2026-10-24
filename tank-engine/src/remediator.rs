use std::fs;
use std::path::Path;
use std::time::Duration;
use clickhouse::Client;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use crate::backfiller::run_historical_backfill;
use crate::types::FixedString66;

const BLOCKSCOUT_BASE_URL: &str = "https://worldchain-mainnet.explorer.alchemy.com/api/v2";
const ETHERSCAN_BASE_URL: &str = "https://api.etherscan.io/v2/api";
/// Etherscan V2 API key, read from the ETHERSCAN_API_KEY environment variable.
fn etherscan_api_key() -> String {
    std::env::var("ETHERSCAN_API_KEY").unwrap_or_default()
}
const WORLDCHAIN_CHAIN_ID: u64 = 480;

const QUOTA_BUDGET_FILE: &str = "/tmp/blockscout_quota.budget";
const DAILY_LIMIT: u64 = 95000;
const CLICKHOUSE_URL: &str = "http://127.0.0.1:8123";
const CLICKHOUSE_DB: &str = "worldchain";

#[derive(Debug, Clone, serde::Serialize, clickhouse::Row)]
pub struct VerificationLogRow {
    pub check_time: u32,
    pub target_table: String,
    pub target_field: String,
    pub entity_type: String,
    pub entity_id: String,
    pub block_number: u64,
    pub clickhouse_val: Option<String>,
    pub etherscan_val: Option<String>,
    pub blockscout_val: Option<String>,
    pub match_status: u8,
    pub diagnostic_notes: String,
}

#[derive(Debug, Deserialize)]
struct BlockscoutBlockResponse {
    pub hash: Option<String>,
    pub gas_used: Option<serde_json::Value>,
    pub tx_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EtherscanBlockResponse {
    pub result: Option<serde_json::Value>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize, clickhouse::Row)]
struct ClickHouseBlockCheck {
    pub block_hash: FixedString66,
    pub gas_used: u64,
    pub transactions_count: u32,
}

fn check_and_increment_quota(cost: u64) -> bool {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    
    let mut current_quota = 0u64;
    if Path::new(QUOTA_BUDGET_FILE).exists() {
        if let Ok(metadata) = fs::metadata(QUOTA_BUDGET_FILE) {
            if let Ok(modified) = metadata.modified() {
                let file_date = chrono::DateTime::<chrono::Utc>::from(modified)
                    .format("%Y-%m-%d")
                    .to_string();
                if file_date == today {
                    if let Ok(content) = fs::read_to_string(QUOTA_BUDGET_FILE) {
                        current_quota = content.trim().parse().unwrap_or(0);
                    }
                }
            }
        }
    }

    if current_quota >= DAILY_LIMIT {
        warn!(
            "[⚠️ QUOTA WARNING] Reached limit ({}/{}). Pausing API audit requests.",
            current_quota, DAILY_LIMIT
        );
        return false;
    }

    let new_quota = current_quota + cost;
    let _ = fs::write(QUOTA_BUDGET_FILE, new_quota.to_string());
    true
}

pub async fn run_remediator_loop(start_block: u64, end_block: u64, tranche_size: u64) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "=== TANK-ENGINE AUDITOR STARTED | Range: {} -> {} | Tranche: {} ===",
        start_block, end_block, tranche_size
    );

    let client = Client::default()
        .with_url(CLICKHOUSE_URL)
        .with_database(CLICKHOUSE_DB);

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let mut current_start = start_block;

    while current_start <= end_block {
        let current_end = (current_start + tranche_size - 1).min(end_block);

        info!("=======================================================");
        info!("--- HEALER TRANCHE: {} -> {} ---", current_start, current_end);
        info!("=======================================================");

        // 1. Native Rust Backfill/Heal Range
        if let Err(e) = run_historical_backfill(current_start, current_end, 1000).await {
            warn!("Remediator backfill tranche error on {}->{}: {}", current_start, current_end, e);
        }

        // 2. Memory settling pause
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 3. Multi-Source Quality Verification Check
        if check_and_increment_quota(10) {
            let mut rng = rand::thread_rng();
            let rand_block = rng.gen_range(current_start..=current_end);
            info!("[🔍 EXPANDED-AUDITOR] Verifying Block {} (hash, gas_used, transactions_count) across APIs...", rand_block);

            // Fetch from Blockscout API
            let bs_url = format!("{}/blocks/{}", BLOCKSCOUT_BASE_URL, rand_block);
            let mut bs_hash: Option<String> = None;
            let mut bs_gas_used: Option<String> = None;
            let mut bs_tx_count: Option<u64> = None;

            match http_client.get(&bs_url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        if let Ok(bs_block) = resp.json::<BlockscoutBlockResponse>().await {
                            bs_hash = bs_block.hash.map(|h| h.to_lowercase());
                            bs_gas_used = bs_block.gas_used.map(|v| v.to_string().trim_matches('"').to_string());
                            bs_tx_count = bs_block.tx_count;
                        } else {
                            warn!("[⚠️ AUDIT SKIPPED] Blockscout JSON parse failed for block {}", rand_block);
                        }
                    } else {
                        warn!("[⚠️ AUDIT SKIPPED] Blockscout returned HTTP status {} for block {}", resp.status(), rand_block);
                    }
                }
                Err(e) => warn!("[⚠️ AUDIT SKIPPED] Blockscout connection failed for block {}: {}", rand_block, e),
            }

            // Fetch from Etherscan V2 API
            let es_hex_block = format!("0x{:x}", rand_block);
            let es_url = format!(
                "{}?chainid={}&module=proxy&action=eth_getBlockByNumber&tag={}&boolean=true&apikey={}",
                ETHERSCAN_BASE_URL, WORLDCHAIN_CHAIN_ID, es_hex_block, etherscan_api_key()
            );

            let mut es_hash: Option<String> = None;
            let mut es_gas_used: Option<String> = None;
            let mut es_tx_count: Option<u64> = None;

            match http_client.get(&es_url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        if let Ok(es_block) = resp.json::<EtherscanBlockResponse>().await {
                            if let Some(ref res_val) = es_block.result {
                                if res_val.is_object() {
                                    if let Some(h) = res_val.get("hash").and_then(|v| v.as_str()) {
                                        es_hash = Some(h.to_lowercase());
                                    }
                                    if let Some(g) = res_val.get("gasUsed").and_then(|v| v.as_str()) {
                                        let clean_hex = g.trim_start_matches("0x");
                                        if let Ok(parsed_gas) = u64::from_str_radix(clean_hex, 16) {
                                            es_gas_used = Some(parsed_gas.to_string());
                                        }
                                    }
                                    if let Some(txs) = res_val.get("transactions").and_then(|v| v.as_array()) {
                                        es_tx_count = Some(txs.len() as u64);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => warn!("[⚠️ AUDIT SKIPPED] Etherscan V2 connection failed for block {}: {}", rand_block, e),
            }

            // Query ClickHouse internal record
            let mut ch_hash = String::new();
            let mut ch_gas_used = 0u64;
            let mut ch_tx_count = 0u64;
            let mut ch_found = false;

            for _attempt in 1..=3 {
                let q = format!(
                    "SELECT block_hash, gas_used, transactions_count FROM worldchain.world_blocks WHERE block_number = {} LIMIT 1",
                    rand_block
                );
                if let Ok(row) = client.query(&q).fetch_one::<ClickHouseBlockCheck>().await {
                    ch_hash = String::from_utf8_lossy(&row.block_hash.0).to_string().to_lowercase();
                    ch_gas_used = row.gas_used;
                    ch_tx_count = row.transactions_count as u64;
                    ch_found = true;
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            if !ch_found {
                warn!("[⚠️ MULTI-AUDITOR] ClickHouse row missing or query failed for block {} after 3 retries.", rand_block);
            } else {
                let timestamp = chrono::Utc::now().timestamp() as u32;
                let mut mismatches = 0;
                let mut verification_logs = Vec::new();

                // Compare Blockscout
                if let Some(ref b_hash) = bs_hash {
                    if &ch_hash != b_hash {
                        mismatches += 1;
                        verification_logs.push(VerificationLogRow {
                            check_time: timestamp,
                            block_number: rand_block,
                            entity_type: "blockscout".to_string(),
                            entity_id: rand_block.to_string(),
                            target_table: "worldchain.blocks".to_string(),
                        target_field: "block_hash".to_string(),
                        match_status: 2,
                        diagnostic_notes: "block_hash mismatch".to_string(),
                            clickhouse_val: Some(ch_hash.clone()),
                            blockscout_val: Some(b_hash.clone()),
                        etherscan_val: None,
                        });
                    }
                }

                if let Some(ref b_gas) = bs_gas_used {
                    if &ch_gas_used.to_string() != b_gas {
                        mismatches += 1;
                        verification_logs.push(VerificationLogRow {
                            check_time: timestamp,
                            block_number: rand_block,
                            entity_type: "blockscout".to_string(),
                            entity_id: rand_block.to_string(),
                            target_table: "worldchain.blocks".to_string(),
                        target_field: "gas_used".to_string(),
                        match_status: 2,
                        diagnostic_notes: "gas_used mismatch".to_string(),
                            clickhouse_val: Some(ch_gas_used.to_string()),
                            blockscout_val: Some(b_gas.clone()),
                        etherscan_val: None,
                        });
                    }
                }

                if let Some(b_tx) = bs_tx_count {
                    if ch_tx_count != b_tx {
                        mismatches += 1;
                        verification_logs.push(VerificationLogRow {
                            check_time: timestamp,
                            block_number: rand_block,
                            entity_type: "blockscout".to_string(),
                            entity_id: rand_block.to_string(),
                            target_table: "worldchain.blocks".to_string(),
                        target_field: "transactions_count".to_string(),
                        match_status: 2,
                        diagnostic_notes: "transactions_count mismatch".to_string(),
                            clickhouse_val: Some(ch_tx_count.to_string()),
                            blockscout_val: Some(b_tx.to_string()),
                        etherscan_val: None,
                        });
                    }
                }

                // Granular multi-provider field audits with type alignment
                let entity_id = rand_block.to_string();

                // 1. Block Hash Audit
                verification_logs.push(audit_field_entry(
                    "worldchain.blocks", "block_hash", "block", &entity_id, rand_block,
                    Some(ch_hash.clone()), es_hash.clone(), bs_hash.clone()
                ));

                // 2. Gas Used Audit (string-aligned)
                let es_gas_u64 = es_gas_used.as_ref().and_then(|s| s.parse::<u64>().ok());
                let bs_gas_u64 = bs_gas_used.as_ref().and_then(|s| s.parse::<u64>().ok());
                verification_logs.push(audit_field_entry(
                    "worldchain.blocks", "gas_used", "block", &entity_id, rand_block,
                    Some(ch_gas_used), es_gas_u64, bs_gas_u64
                ));

                // 3. Transactions Count Audit (u64-aligned)
                let es_tx_u64 = es_tx_count;
                let bs_tx_u64 = bs_tx_count;
                verification_logs.push(audit_field_entry(
                    "worldchain.blocks", "transactions_count", "block", &entity_id, rand_block,
                    Some(ch_tx_count), es_tx_u64, bs_tx_u64
                ));

                info!("[💯 FIELD-AUDIT] Recorded granular field verifications for block {}", rand_block);
                // Write verification logs
                if match client.insert("worldchain.verification_logs_v2") {
                    Ok(mut insert) => {
                        for log_row in verification_logs {
                            let _ = insert.write(&log_row).await;
                        }
                        insert.end().await.is_ok()
                    },
                    Err(_) => false,
                } {}
            }
        }

        current_start = current_end + 1;
    }

    info!("=== REMEDIATOR WORKFLOW COMPLETED SUCCESSFULLY ===");
    Ok(())
}


/// Generates granular field-level verification log rows across multi-source providers
pub fn audit_field_entry<T: PartialEq + std::fmt::Display>(
    target_table: &str,
    target_field: &str,
    entity_type: &str,
    entity_id: &str,
    block_number: u64,
    clickhouse_val: Option<T>,
    etherscan_val: Option<T>,
    blockscout_val: Option<T>,
) -> VerificationLogRow {
    let ch_str = clickhouse_val.as_ref().map(|v| v.to_string());
    let es_str = etherscan_val.as_ref().map(|v| v.to_string());
    let bs_str = blockscout_val.as_ref().map(|v| v.to_string());

    let (match_status, diagnostic_notes) = match (&ch_str, &es_str, &bs_str) {
        (Some(c), Some(e), Some(b)) if c == e && c == b => (1u8, "BS + ETHERSCAN VERIFIED".to_string()),
        (Some(c), Some(e), Some(b)) if e == b && c != e => (2u8, format!("CLICKHOUSE_OUT_OF_SYNC: {}", target_field)),
        (Some(_), Some(e), Some(b)) if e != b => (3u8, format!("EXPLORER_DISCREPANCY: {}", target_field)),
        _ => (4u8, format!("PARTIAL_DATA_OR_TIMEOUT: {}", target_field)),
    };

    VerificationLogRow {
        check_time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32,
        target_table: target_table.to_string(),
        target_field: target_field.to_string(),
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        block_number,
        clickhouse_val: ch_str,
        etherscan_val: es_str,
        blockscout_val: bs_str,
        match_status,
        diagnostic_notes,
    }
}

