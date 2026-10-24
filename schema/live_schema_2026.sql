CREATE DATABASE IF NOT EXISTS worldchain ENGINE = Atomic;

-- =========================================================================
-- 1. ANALYTICS & API TABLES
-- =========================================================================

CREATE TABLE IF NOT EXISTS worldchain.api_dex_swaps (
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`log_index` UInt32 CODEC(ZSTD(3)),
`pool_address` FixedString(42) CODEC(ZSTD(3)),
`sender` FixedString(42) CODEC(ZSTD(3)),
`recipient` FixedString(42) CODEC(ZSTD(3)),
`amount0` Int256 CODEC(ZSTD(3)),
`amount1` Int256 CODEC(ZSTD(3)),
`sqrtPriceX96` UInt256 CODEC(ZSTD(3)),
`liquidity` UInt256 CODEC(ZSTD(3)),
`tick` Int32 CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(toDate(ingested_at)) ORDER BY (block_number, tx_hash, log_index) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_gas_sponsorship (
`ingested_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`human_address` FixedString(42) CODEC(ZSTD(3)),
`paymaster_address` FixedString(42) CODEC(ZSTD(3)),
`gas_cost_hex` String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(ingested_at) ORDER BY (ingested_at, paymaster_address) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_humanity_feed (
`ingested_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`human_address` FixedString(42) CODEC(ZSTD(3)),
`action_type` LowCardinality(String) CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(ingested_at) ORDER BY (human_address, ingested_at) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_internal_transfers (
`ingested_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`from_address` FixedString(42) CODEC(ZSTD(3)),
`to_address` FixedString(42) CODEC(ZSTD(3)),
`value_wei` UInt256 CODEC(ZSTD(3)),
`depth` UInt32 CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(ingested_at) ORDER BY (to_address, ingested_at) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_live_dex_swaps (
`ingested_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`pool_address` FixedString(42) CODEC(ZSTD(3)),
`is_v2` UInt8 CODEC(ZSTD(3)),
`is_v3` UInt8 CODEC(ZSTD(3)),
`raw_data` String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(ingested_at) ORDER BY (pool_address, ingested_at) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_oracle_prices (
`ingested_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`token_symbol` String CODEC(ZSTD(3)),
`price_usd` Float64 CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(ingested_at) ORDER BY (token_symbol, ingested_at) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_uniswap_v3_prices (
`ingested_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`pool_address` FixedString(42) CODEC(ZSTD(3)),
`sqrt_price_x96` UInt256 CODEC(ZSTD(3)),
`price_raw` Float64 CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(ingested_at) ORDER BY (pool_address, ingested_at) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_universal_defi_events (
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`log_index` UInt32 CODEC(ZSTD(3)),
`contract_address` FixedString(42) CODEC(ZSTD(3)),
`event_type` LowCardinality(String) CODEC(ZSTD(3)),
`raw_data` String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(toDate(ingested_at)) ORDER BY (block_number, tx_hash, log_index) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_whale_alerts (
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`log_index` UInt32 CODEC(ZSTD(3)),
`token_address` FixedString(42) CODEC(ZSTD(3)),
`from_address` FixedString(42) CODEC(ZSTD(3)),
`to_address` FixedString(42) CODEC(ZSTD(3)),
`value_raw` String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(toDate(ingested_at)) ORDER BY (block_number, tx_hash, log_index) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.dex_pairs (
`chain` String DEFAULT 'worldchain' CODEC(ZSTD(3)),
`chainId` String DEFAULT '480' CODEC(ZSTD(3)),
`pairAddress` String CODEC(ZSTD(3)),
`dexId` String CODEC(ZSTD(3)),
`url` String CODEC(ZSTD(3)),
`baseToken_address` String CODEC(ZSTD(3)),
`baseToken_name` String CODEC(ZSTD(3)),
`baseToken_symbol` String CODEC(ZSTD(3)),
`quoteToken_symbol` String CODEC(ZSTD(3)),
`priceNative` Float64 CODEC(ZSTD(3)),
`priceUsd` Float64 CODEC(ZSTD(3)),
`fetch_type` Enum8('liquidity' = 1, 'hardcoded' = 2, 'api' = 3, 'manual' = 4) CODEC(ZSTD(3)),
`timestamp` DateTime64(3) DEFAULT now64(3) CODEC(ZSTD(3)),
`version` UInt64 CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree(version) ORDER BY (chain, pairAddress) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.healer_checkpoints (
`id` String,
`last_verified_block` UInt64,
`updated_at` DateTime64(3) DEFAULT now()
) ENGINE = ReplacingMergeTree ORDER BY id SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_abi_signatures (
`selector_or_topic` FixedString(32),
`text_signature` String,
`kind` LowCardinality(String),
`source` LowCardinality(String),
`updated_at` DateTime DEFAULT now()
) ENGINE = ReplacingMergeTree ORDER BY selector_or_topic SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_address_labels (
`chain_id` UInt32 DEFAULT 480,
`address` FixedString(42),
`label` String,
`label_category` LowCardinality(String),
`source` LowCardinality(String),
`world_id_flag` UInt8 DEFAULT 0,
`updated_at` DateTime DEFAULT now()
) ENGINE = ReplacingMergeTree ORDER BY (chain_id, address) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_audit_logs (
`timestamp` DateTime CODEC(DoubleDelta, ZSTD(1)),
`topic` LowCardinality(String) CODEC(ZSTD(1)),
`audit_name` LowCardinality(String) CODEC(ZSTD(1)),
`status` Enum8('OK' = 1, 'WARN' = 2, 'FAIL' = 3) CODEC(ZSTD(1)),
`findings_count` UInt32 CODEC(T64, ZSTD(1)),
`raw_output` String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMM(timestamp) ORDER BY (topic, status, timestamp) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_dex_trades (
`chain_id` UInt32,
`block_number` UInt64,
`timestamp` DateTime,
`tx_hash` FixedString(32),
`log_index` UInt32,
`pair_address` FixedString(20),
`maker` FixedString(20),
`token_in` FixedString(20),
`token_out` FixedString(20),
`amount_in` Float64,
`amount_out` Float64,
`price_native` Float64,
`price_usd` Float64
) ENGINE = ReplacingMergeTree ORDER BY (pair_address, timestamp) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_token_metadata (
`chain_id` UInt32 DEFAULT 480,
`contract_address` FixedString(42),
`standard` LowCardinality(String),
`symbol` String,
`name` String,
`decimals` UInt8,
`first_seen_block` UInt64,
`verified` UInt8 DEFAULT 0,
`updated_at` DateTime DEFAULT now()
) ENGINE = ReplacingMergeTree ORDER BY (chain_id, contract_address) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_token_prices (
`token_address` FixedString(42),
`minute_bucket` DateTime,
`price_usd` Float64,
`liquidity_usd` Float64,
`source_pair` FixedString(42)
) ENGINE = ReplacingMergeTree ORDER BY (token_address, minute_bucket) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_token_transfers (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`log_index` UInt32 CODEC(ZSTD(3)),
`token_address` FixedString(42) CODEC(ZSTD(3)),
`from_address` FixedString(42) CODEC(ZSTD(3)),
`to_address` FixedString(42) CODEC(ZSTD(3)),
`value` String CODEC(ZSTD(3)),
`token_id` String CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) DEFAULT now() CODEC(DoubleDelta, ZSTD(3))
) ENGINE = ReplacingMergeTree ORDER BY (block_number, tx_hash, log_index) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_token_balances (
`wallet_address` String CODEC(ZSTD(3)),
`token_address` String CODEC(ZSTD(3)),
`balance_change` Int256 CODEC(Delta(8), ZSTD(3))
) ENGINE = SummingMergeTree ORDER BY (wallet_address, token_address) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_network_metrics (
`day` Date CODEC(Delta(2), ZSTD(3)),
`active_users` AggregateFunction(uniq, String) CODEC(ZSTD(3)),
`total_txs` AggregateFunction(count, UInt64) CODEC(ZSTD(3))
) ENGINE = AggregatingMergeTree ORDER BY day SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_nft_transfers (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`contract_address` String CODEC(ZSTD(3)),
`token_id` UInt256 CODEC(ZSTD(3)),
`from_address` String CODEC(ZSTD(3)),
`to_address` String CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3))
) ENGINE = ReplacingMergeTree ORDER BY (contract_address, token_id, block_number) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_lending_liquidations (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` String CODEC(ZSTD(3)),
`liquidator` String CODEC(ZSTD(3)),
`borrower` String CODEC(ZSTD(3)),
`collateral_asset` String CODEC(ZSTD(3)),
`debt_asset` String CODEC(ZSTD(3)),
`debt_covered` Int256 CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3))
) ENGINE = ReplacingMergeTree ORDER BY (tx_hash, borrower) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_dex_hourly_stats (
`hour` DateTime CODEC(Delta(4), ZSTD(3)),
`pool_address` String CODEC(ZSTD(3)),
`hourly_swap_count` AggregateFunction(count, UInt64) CODEC(ZSTD(3)),
`last_trade_time` AggregateFunction(argMax, DateTime64(3), DateTime64(3)) CODEC(ZSTD(3))
) ENGINE = AggregatingMergeTree ORDER BY (pool_address, hour) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_morpho_activity (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`tx_hash` String CODEC(ZSTD(3)),
`market_id` String CODEC(ZSTD(3)),
`action_type` Enum8('Borrow' = 1, 'Repay' = 2, 'Liquidate' = 3) CODEC(ZSTD(3)),
`user_address` String CODEC(ZSTD(3)),
`assets_amount` Int256 CODEC(ZSTD(3)),
`shares_amount` Int256 CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3))
) ENGINE = ReplacingMergeTree ORDER BY (market_id, tx_hash, action_type) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_uniswap_positions (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`pool_address` String CODEC(ZSTD(3)),
`tick_lower` Int32 CODEC(ZSTD(3)),
`tick_upper` Int32 CODEC(ZSTD(3)),
`liquidity_minted` Int256 CODEC(ZSTD(3)),
`owner` String CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3))
) ENGINE = ReplacingMergeTree ORDER BY (pool_address, tick_lower, tick_upper, block_number) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_native_microloans (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`protocol` String CODEC(ZSTD(3)),
`world_id_hash` String CODEC(ZSTD(3)),
`loan_amount_usd` Int256 CODEC(ZSTD(3)),
`status` Enum8('Issued' = 1, 'Defaulted' = 2, 'Repaid' = 3) CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3))
) ENGINE = ReplacingMergeTree ORDER BY (protocol, world_id_hash) SETTINGS index_granularity = 8192;

-- =========================================================================
-- 2. THE 4 CORE L2 TABLES
-- Last updated: 2026-03-14
--   - trace_index added to world_traces (ALTER: 2026-03-14)
-- =========================================================================

-- BLOCKS TABLE
DROP TABLE IF EXISTS worldchain.world_blocks;
CREATE TABLE worldchain.world_blocks (
`chain_id` UInt32 CODEC(ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`block_hash` FixedString(66) CODEC(ZSTD(3)),
`parent_hash` FixedString(66) CODEC(ZSTD(3)),
`timestamp` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`miner` FixedString(42) CODEC(ZSTD(3)),
`gas_limit` UInt64 CODEC(ZSTD(3)),
`gas_used` UInt64 CODEC(ZSTD(3)),
`base_fee_per_gas` UInt64 CODEC(Delta(8), ZSTD(3)),
`transactions_count` UInt32 CODEC(ZSTD(3)),
`size_bytes` UInt32 CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (chain_id, block_number, block_hash)
SETTINGS index_granularity = 8192;

-- TRANSACTIONS TABLE
DROP TABLE IF EXISTS worldchain.world_transactions;
CREATE TABLE worldchain.world_transactions (
`chain_id` UInt32 CODEC(ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`block_hash` FixedString(66) CODEC(ZSTD(3)),
`tx_index` UInt32 CODEC(ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`from_address` FixedString(42) CODEC(ZSTD(3)),
`to_address` FixedString(42) CODEC(ZSTD(3)),
`value_wei` UInt256 CODEC(ZSTD(3)),
`mint_wei` UInt256 CODEC(ZSTD(3)),
`gas_price_wei` UInt64 CODEC(ZSTD(3)),
`max_fee_per_gas_wei` UInt64 CODEC(ZSTD(3)),
`max_priority_fee_wei` UInt64 CODEC(ZSTD(3)),
`gas_limit` UInt64 CODEC(ZSTD(3)),
`gas_used` UInt64 CODEC(ZSTD(3)),
`nonce` UInt64 CODEC(ZSTD(3)),
`tx_type` UInt8 CODEC(ZSTD(3)),
`is_system_tx` UInt8 CODEC(ZSTD(3)),
`input_data` String CODEC(ZSTD(3)),
`status` UInt8 CODEC(ZSTD(3)),
`effective_gas_price_wei` UInt64 CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`l1_fee_wei` UInt256 CODEC(ZSTD(3)),
`l1_gas_price_wei` UInt64 CODEC(ZSTD(3)),
`is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
`healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3)),
INDEX idx_from from_address TYPE bloom_filter GRANULARITY 4,
INDEX idx_to to_address TYPE bloom_filter GRANULARITY 4
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (chain_id, block_number, tx_index, tx_hash)
SETTINGS index_granularity = 8192;

-- LOGS TABLE
DROP TABLE IF EXISTS worldchain.world_logs;
CREATE TABLE worldchain.world_logs (
`chain_id` UInt32 CODEC(ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`block_hash` FixedString(66) CODEC(ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`tx_index` UInt32 CODEC(ZSTD(3)),
`log_index` UInt32 CODEC(ZSTD(3)),
`address` FixedString(42) CODEC(ZSTD(3)),
`data` String CODEC(ZSTD(3)),
`topics` Array(FixedString(66)) CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
`healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3)),
INDEX idx_log_address address TYPE bloom_filter GRANULARITY 4
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (chain_id, block_number, tx_index, log_index)
SETTINGS index_granularity = 8192;

-- TRACES TABLE
-- trace_index column added 2026-03-14 via:
--   ALTER TABLE worldchain.world_traces ADD COLUMN `trace_index` UInt32 DEFAULT 0 CODEC(ZSTD(3));
DROP TABLE IF EXISTS worldchain.world_traces;
CREATE TABLE worldchain.world_traces (
`chain_id` UInt32 CODEC(ZSTD(3)),
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`block_hash` FixedString(66) CODEC(ZSTD(3)),
`tx_index` UInt32 CODEC(ZSTD(3)),
`tx_hash` FixedString(66) CODEC(ZSTD(3)),
`from_address` FixedString(42) CODEC(ZSTD(3)),
`to_address` FixedString(42) CODEC(ZSTD(3)),
`value_wei` UInt256 CODEC(ZSTD(3)),
`input_data` String CODEC(ZSTD(3)),
`output_data` String CODEC(ZSTD(3)),
`gas_limit` UInt64 CODEC(ZSTD(3)),
`gas_used` UInt64 CODEC(ZSTD(3)),
`trace_type` String CODEC(ZSTD(3)),
`call_type` String CODEC(ZSTD(3)),
`error` String CODEC(ZSTD(3)),
`depth` UInt32 CODEC(ZSTD(3)),
`ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
`trace_index` UInt32 DEFAULT 0 CODEC(ZSTD(3)),
INDEX idx_trace_from from_address TYPE bloom_filter GRANULARITY 4,
INDEX idx_trace_to to_address TYPE bloom_filter GRANULARITY 4
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (chain_id, block_number, tx_index, depth)
SETTINGS index_granularity = 8192;

-- =========================================================================
-- 3. TRACE GAP LEDGER — tracks blocks where trace fetch failed
-- Added: 2026-03-14 — military-grade gap tracking for retrier
-- =========================================================================

CREATE TABLE IF NOT EXISTS worldchain.world_trace_gaps (
`block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
`reason` String CODEC(ZSTD(3)),
`rpc_error_code` Int32 DEFAULT 0 CODEC(ZSTD(3)),
`attempted_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
`resolved` UInt8 DEFAULT 0 CODEC(ZSTD(3)),
`resolved_at` DateTime64(3) DEFAULT toDateTime64(0, 3) CODEC(DoubleDelta, ZSTD(3))
) ENGINE = ReplacingMergeTree(attempted_at)
ORDER BY (block_number, rpc_error_code)
SETTINGS index_granularity = 8192;

-- =========================================================================
-- 4. MATERIALIZED VIEWS & REGULAR VIEWS
-- =========================================================================

CREATE OR REPLACE VIEW worldchain.erc20_transfers_view AS SELECT l.block_number, l.tx_hash AS transaction_hash, l.address AS token_address, concat('0x', substring(l.topics[2], 27, 40)) AS from_address, concat('0x', substring(l.topics[3], 27, 40)) AS to_address, reinterpretAsUInt256(reverse(unhex(substring(l.data, 3)))) AS amount_raw, toUInt256(amount_raw) / 1000000000000000000. AS amount_normalized, b.timestamp FROM worldchain.world_logs AS l LEFT JOIN worldchain.world_blocks AS b ON l.block_number = b.block_number WHERE (l.topics[1]) = '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef';

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_dex_swaps TO worldchain.api_dex_swaps AS SELECT now64(3) AS ingested_at, block_number, tx_hash, log_index, address AS pool_address, concat('0x', substring(topics[2], 27, 40)) AS sender, concat('0x', substring(topics[3], 27, 40)) AS recipient, reinterpretAsInt256(reverse(unhex(substring(data, 3, 64)))) AS amount0, reinterpretAsInt256(reverse(unhex(substring(data, 67, 64)))) AS amount1, reinterpretAsUInt256(reverse(unhex(substring(data, 131, 64)))) AS sqrtPriceX96, reinterpretAsUInt256(reverse(unhex(substring(data, 195, 64)))) AS liquidity, reinterpretAsInt32(reverse(unhex(substring(data, 259, 64)))) AS tick FROM worldchain.world_logs WHERE (topics[1]) = '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67';

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_gas_sponsorship TO worldchain.api_gas_sponsorship AS SELECT toDateTime(ingested_at) AS ingested_at, block_number, tx_hash, concat('0x', substring(topics[3], 27, 40)) AS human_address, concat('0x', substring(topics[4], 27, 40)) AS paymaster_address, substring(data, 131, 64) AS gas_cost_hex FROM worldchain.world_logs WHERE (topics[1]) = '0x49628fd1471006c1482da88028e9ce4dbb080b815c9b0344d39e5a8e6ec1419f';

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_humanity_feed TO worldchain.api_humanity_feed AS SELECT toDateTime(l.ingested_at) AS ingested_at, l.block_number, l.tx_hash, concat('0x', substring(l.topics[2], 27, 40)) AS human_address, 'WORLD_ID_VERIFIED' AS action_type FROM worldchain.world_logs AS l WHERE (l.address = '0x17B354dD2595411ff79041f930e491A4Df39A278') AND ((l.topics[1]) = '0x4bd07c08a95610ec871ff21503c2ff5ddc0060fb8584dc8263ca7b52479e0a0a');

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_internal_transfers TO worldchain.api_internal_transfers AS SELECT toDateTime(ingested_at) AS ingested_at, block_number, tx_hash, from_address, to_address, value_wei, depth FROM worldchain.world_traces WHERE (trace_type IN ('call', 'CALL')) AND (value_wei > 0) AND (depth > 0);

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_live_dex_swaps TO worldchain.api_live_dex_swaps AS SELECT toDateTime(ingested_at) AS ingested_at, block_number, tx_hash, address AS pool_address, (topics[1]) = '0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822' AS is_v2, (topics[1]) = '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67' AS is_v3, data AS raw_data FROM worldchain.world_logs WHERE (topics[1]) IN ('0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822', '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67');

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_oracle_prices TO worldchain.api_oracle_prices AS SELECT toDateTime(ingested_at) AS ingested_at, block_number, multiIf(address = '0x8Bb2943AB030E3eE05a58d9832525B4f60A97FA0', 'WLD', address = '0xe1d72a719171DceAB9499757EB9d5AEb9e8D64A6', 'ETH', address = '0xF4301686AfF4eE36d70c718a9e62309b53862BE8', 'USDC', address) AS token_symbol, reinterpretAsFloat64(unhex(substring(data, 3, 64))) / 100000000 AS price_usd FROM worldchain.world_logs WHERE (topics[1]) IN ('0x0559884fd3a46039e246a27e7cb3f1a1c09d8bf002a79246102aa6bc4d7c58ed', '0x0559884fd3a460db30fea510563813e07234719452a4306a4e3e481b29a285d3');

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_uniswap_v3_prices TO worldchain.api_uniswap_v3_prices AS SELECT toDateTime(ingested_at) AS ingested_at, block_number, tx_hash, address AS pool_address, reinterpretAsUInt256(reverse(unhex(substring(data, 131, 64)))) AS sqrt_price_x96, pow(sqrt_price_x96 / 7.922816251426434e28, 2) AS price_raw FROM worldchain.world_logs WHERE (topics[1]) = '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67';

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_universal_defi_events TO worldchain.api_universal_defi_events AS SELECT now64(3) AS ingested_at, block_number, tx_hash, log_index, address AS contract_address, multiIf(address = '0x17B354dD2595411ff79041f930e491A4Df39A278', 'WORLD_ID_VERIFICATION', address = '0x4200000000000000000000000000000000000010', 'OFFICIAL_BRIDGE_TRANSFER', (topics[1]) = '0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822', 'V2_SWAP', (topics[1]) = '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67', 'V3_SWAP', (topics[1]) = '0xcc267713d782161bc7855e903bc701fbfa1fbc8f5b4970425ccfb4e578c7eb46', 'V4_SWAP', 'UNKNOWN') AS event_type, data AS raw_data FROM worldchain.world_logs WHERE (address IN ('0x17B354dD2595411ff79041f930e491A4Df39A278', '0x4200000000000000000000000000000000000010')) OR ((topics[1]) IN ('0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822', '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67', '0xcc267713d782161bc7855e903bc701fbfa1fbc8f5b4970425ccfb4e578c7eb46'));

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_whale_alerts TO worldchain.api_whale_alerts AS SELECT now64(3) AS ingested_at, block_number, tx_hash, log_index, token_address, from_address, to_address, value AS value_raw FROM worldchain.world_token_transfers WHERE length(value) > 22;

CREATE OR REPLACE VIEW worldchain.view_human_analytics AS SELECT toStartOfDay(ingested_at) AS day, countDistinct(topics[2]) AS unique_users, count() AS total_interactions FROM worldchain.world_logs WHERE (topics[1]) = '0x0000000000000000000000000000000000000000000000000000000000000001' GROUP BY day;

CREATE OR REPLACE VIEW worldchain.view_token_transfers AS SELECT l.chain_id, l.block_number, l.tx_hash, l.log_index, l.address AS token_address, concat('0x', substring(l.topics[2], 27, 40)) AS from_address, concat('0x', substring(l.topics[3], 27, 40)) AS to_address, multiIf((l.data != '0x') AND (length(l.data) >= 66), reinterpretAsUInt256(unhex(substring(l.data, 3))), length(l.topics) >= 4, reinterpretAsUInt256(unhex(substring(l.topics[4], 3))), 0) AS raw_amount FROM worldchain.world_logs AS l WHERE ((l.topics[1]) = '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef') AND (l.is_canonical = 1);

CREATE OR REPLACE VIEW worldchain.view_verified_human_activity AS SELECT t.block_number, t.tx_hash, t.from_address AS smart_wallet_address, multiIf(l.label = 'WorldIDRouter', 'Verification', l.label LIKE '%Grant%', 'Claim', 'Transaction') AS human_action, t.gas_used FROM worldchain.world_transactions AS t INNER JOIN worldchain.world_address_labels AS l ON l.address = t.to_address WHERE (l.label_category IN ('Identity', 'Grants')) AND (t.is_canonical = 1);

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_token_balances TO worldchain.api_token_balances AS SELECT ledger.1 AS wallet_address, address AS token_address, ledger.2 AS balance_change FROM worldchain.world_logs ARRAY JOIN [tuple(concat('0x', substring(topics[2], 27, 40)), -reinterpretAsInt256(reverse(unhex(substring(data, 3, 64))))), tuple(concat('0x', substring(topics[3], 27, 40)), reinterpretAsInt256(reverse(unhex(substring(data, 3, 64)))))] AS ledger WHERE ((topics[1]) = '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef') AND (length(topics) >= 3) AND (length(data) >= 66);

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_nft_transfers TO worldchain.api_nft_transfers AS SELECT block_number, address AS contract_address, reinterpretAsUInt256(reverse(unhex(substring(topics[4], 3, 64)))) AS token_id, concat('0x', substring(topics[2], 27, 40)) AS from_address, concat('0x', substring(topics[3], 27, 40)) AS to_address, now64(3) AS ingested_at FROM worldchain.world_logs WHERE ((topics[1]) = '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef') AND (length(topics) = 4);

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_lending_liquidations TO worldchain.api_lending_liquidations AS SELECT block_number, tx_hash, concat('0x', substring(topics[2], 27, 40)) AS collateral_asset, concat('0x', substring(topics[3], 27, 40)) AS debt_asset, concat('0x', substring(topics[4], 27, 40)) AS borrower, concat('0x', substring(data, 195, 40)) AS liquidator, reinterpretAsInt256(reverse(unhex(substring(data, 3, 64)))) AS debt_covered, now64(3) AS ingested_at FROM worldchain.world_logs WHERE (topics[1]) = '0xe413a321e868d831f4dbccbca790d2952b56f977908e45be3733527b14066928';

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_dex_hourly_stats TO worldchain.api_dex_hourly_stats AS SELECT toStartOfHour(ingested_at) AS hour, pool_address, countState(toUInt64(1)) AS hourly_swap_count, argMaxState(ingested_at, ingested_at) AS last_trade_time FROM worldchain.api_dex_swaps GROUP BY hour, pool_address;

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_morpho_borrows TO worldchain.api_morpho_activity AS SELECT block_number, tx_hash, concat('0x', substring(topics[2], 27, 40)) AS market_id, CAST('Borrow', 'Enum8(''Borrow'' = 1, ''Repay'' = 2, ''Liquidate'' = 3)') AS action_type, concat('0x', substring(topics[3], 27, 40)) AS user_address, reinterpretAsInt256(reverse(unhex(substring(data, 3, 64)))) AS assets_amount, reinterpretAsInt256(reverse(unhex(substring(data, 67, 64)))) AS shares_amount, now64(3) AS ingested_at FROM worldchain.world_logs WHERE (topics[1]) = '0x3b070ba26e38605c31f4ff68f12128710ac2ea1deebac3b07eb11d9bc18dbd80';

CREATE MATERIALIZED VIEW IF NOT EXISTS worldchain.mv_api_uniswap_mints TO worldchain.api_uniswap_positions AS SELECT block_number, address AS pool_address, reinterpretAsInt32(reverse(unhex(substring(topics[3], 3, 64)))) AS tick_lower, reinterpretAsInt32(reverse(unhex(substring(topics[4], 3, 64)))) AS tick_upper, reinterpretAsInt256(reverse(unhex(substring(data, 3, 64)))) AS liquidity_minted, concat('0x', substring(topics[2], 27, 40)) AS owner, now64(3) AS ingested_at FROM worldchain.world_logs WHERE (topics[1]) = '0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde';
