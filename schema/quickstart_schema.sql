-- Quick-start schema for a local, empty ClickHouse instance.
--
--   clickhouse-client --multiquery < schema/quickstart_schema.sql
--
-- One file, creates the `worldchain` database and every table that rpc-gateway,
-- api, sgip and mev-detector reference (the table list and the source references
-- are in schema/README.md). Every statement is CREATE ... IF NOT EXISTS, so the
-- file can be re-applied safely; re-applying does not alter existing tables
-- (it will NOT migrate an older table to a changed definition).
--
-- Provenance: the column, engine, ORDER BY and PARTITION BY definitions are taken
-- unchanged from the table DDL already published in schema/complete_schema.sql
-- (a raw SHOW CREATE dump, which does not apply as-is). Mechanical changes only:
--   * CREATE TABLE -> CREATE TABLE IF NOT EXISTS;
--   * dump escaping (literal \n and \') decoded;
--   * deployment-specific settings removed: storage_policy, every TTL clause (row expiry
--     and tiering are deployment policy), non_replicated_deduplication_window and other
--     merge/part tuning settings, so the file applies on a default single-node install;
--   * only the tables the services need are included (no backup, repair, staging or
--     view objects from the dump). Projections from the dump are kept.
-- Not included: materialized views that populate the api_* tables, dictionaries and
-- any data. The api_* tables are created empty and nothing here fills them.

CREATE DATABASE IF NOT EXISTS worldchain;

-- ---- Core indexed-chain tables ----

CREATE TABLE IF NOT EXISTS worldchain.world_blocks
(
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
    `is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
    `healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3))
)
ENGINE = ReplacingMergeTree
ORDER BY (chain_id, block_number, block_hash)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_transactions
(
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
    `block_timestamp` DateTime CODEC(DoubleDelta, ZSTD(3)),
    `l1_fee_wei` UInt256 CODEC(ZSTD(3)),
    `l1_gas_price_wei` UInt64 CODEC(ZSTD(3)),
    `is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
    `healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3)),
    INDEX idx_tx_from from_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_tx_to to_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_tx_timestamp block_timestamp TYPE minmax GRANULARITY 1
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(block_timestamp)
ORDER BY (chain_id, block_number, tx_index, tx_hash)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_logs
(
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
    `block_timestamp` DateTime CODEC(DoubleDelta, ZSTD(3)),
    `is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
    `healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3)),
    INDEX idx_logs_address address TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_logs_topic1 topics[1] TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_logs_timestamp block_timestamp TYPE minmax GRANULARITY 1,
    PROJECTION proj_by_topic1
    (
        SELECT *
        ORDER BY
            topics[1],
            block_number
    )
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(block_timestamp)
ORDER BY (chain_id, block_number, tx_index, log_index)
SETTINGS index_granularity = 8192, deduplicate_merge_projection_mode = 'rebuild';

CREATE TABLE IF NOT EXISTS worldchain.world_traces
(
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
    `gas_limit` UInt64 CODEC(Delta(8), ZSTD(3)),
    `gas_used` UInt64 CODEC(Delta(8), ZSTD(3)),
    `trace_type` LowCardinality(String) CODEC(ZSTD(3)),
    `call_type` LowCardinality(String) CODEC(ZSTD(3)),
    `error` LowCardinality(String) CODEC(ZSTD(3)),
    `depth` UInt32 CODEC(Delta(4), ZSTD(3)),
    `ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
    `block_timestamp` DateTime CODEC(DoubleDelta, ZSTD(3)),
    `is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
    `trace_index` UInt32 DEFAULT 0 CODEC(Delta(4), ZSTD(3)),
    `healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3)),
    INDEX idx_trace_from from_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_trace_to to_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_trace_timestamp block_timestamp TYPE minmax GRANULARITY 1
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(block_timestamp)
ORDER BY (chain_id, block_number, tx_index, trace_index)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_token_transfers
(
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `tx_hash` FixedString(66) CODEC(ZSTD(3)),
    `log_index` UInt32 CODEC(ZSTD(3)),
    `token_address` FixedString(42) CODEC(ZSTD(3)),
    `from_address` FixedString(42) CODEC(ZSTD(3)),
    `to_address` FixedString(42) CODEC(ZSTD(3)),
    `value` String CODEC(ZSTD(3)),
    `token_id` String CODEC(ZSTD(3)),
    `ingested_at` DateTime64(3) DEFAULT now() CODEC(DoubleDelta, ZSTD(3)),
    `block_timestamp` DateTime CODEC(DoubleDelta, ZSTD(3)),
    INDEX idx_transfers_token token_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_transfers_from from_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_transfers_to to_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_transfers_timestamp block_timestamp TYPE minmax GRANULARITY 1,
    PROJECTION proj_by_from
    (
        SELECT *
        ORDER BY
            from_address,
            block_number
    ),
    PROJECTION proj_by_to
    (
        SELECT *
        ORDER BY
            to_address,
            block_number
    )
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(block_timestamp)
ORDER BY (block_number, tx_hash, log_index)
SETTINGS index_granularity = 8192, deduplicate_merge_projection_mode = 'rebuild';

CREATE TABLE IF NOT EXISTS worldchain.world_address_labels
(
    `chain_id` UInt32 DEFAULT 480,
    `address` FixedString(42),
    `label` String,
    `label_category` LowCardinality(String),
    `source` LowCardinality(String),
    `world_id_flag` UInt8 DEFAULT 0,
    `updated_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree
ORDER BY (chain_id, address)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_abi_signatures
(
    `selector_or_topic` FixedString(32),
    `text_signature` String,
    `kind` LowCardinality(String),
    `source` LowCardinality(String),
    `updated_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree
ORDER BY selector_or_topic
SETTINGS index_granularity = 8192;

-- ---- Tables the services query that are not part of the core set ----

CREATE TABLE IF NOT EXISTS worldchain.world_contracts
(
    `chain_id` UInt32 DEFAULT 480 CODEC(ZSTD(1)),
    `address` FixedString(42) CODEC(ZSTD(3)),
    `contract_name` LowCardinality(String) CODEC(ZSTD(3)),
    `eth_balance_wei` String CODEC(ZSTD(1)),
    `tx_count` UInt64 DEFAULT 0 CODEC(ZSTD(3)),
    `creator_address` FixedString(42) CODEC(ZSTD(3)),
    `creator_tx_hash` FixedString(66) CODEC(ZSTD(3)),
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `deployed_at` DateTime CODEC(DoubleDelta, ZSTD(3)),
    `is_verified` UInt8 DEFAULT 0 CODEC(T64, ZSTD(1)),
    `verified_at` Nullable(DateTime) CODEC(ZSTD(1)),
    `verification_source` LowCardinality(String) CODEC(ZSTD(1)),
    `license_type` LowCardinality(String) CODEC(ZSTD(1)),
    `audit_status` LowCardinality(String) CODEC(ZSTD(1)),
    `audit_url` String CODEC(ZSTD(3)),
    `similar_contract` String CODEC(ZSTD(3)),
    `compiler_version` LowCardinality(String) CODEC(ZSTD(3)),
    `optimization_enabled` UInt8 DEFAULT 0 CODEC(T64, ZSTD(1)),
    `optimization_runs` UInt32 DEFAULT 0 CODEC(ZSTD(1)),
    `evm_version` LowCardinality(String) CODEC(ZSTD(1)),
    `is_proxy` UInt8 DEFAULT 0 CODEC(T64, ZSTD(1)),
    `proxy_impl_address` Nullable(FixedString(42)) CODEC(ZSTD(3)),
    `abi` String CODEC(ZSTD(9)),
    `source_code` String CODEC(ZSTD(9)),
    `standard_json_input` String CODEC(ZSTD(9)),
    `file_tree_json` String CODEC(ZSTD(6)),
    `creation_code` String CODEC(ZSTD(6)),
    `deployed_bytecode` String CODEC(ZSTD(6)),
    `constructor_args` String CODEC(ZSTD(3)),
    `is_decompiled` UInt8 DEFAULT 0 CODEC(T64, ZSTD(1)),
    `cfg_graph` String CODEC(ZSTD(9)),
    `disassembly` String CODEC(ZSTD(6)),
    `ingested_at` DateTime64(3) DEFAULT now64(3) CODEC(DoubleDelta, ZSTD(3)),
    `deployer` FixedString(42) DEFAULT creator_address CODEC(ZSTD(3)),
    INDEX idx_contract_address address TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(ingested_at)
PARTITION BY toYYYYMM(deployed_at)
PRIMARY KEY (chain_id, deployed_at, address)
ORDER BY (chain_id, deployed_at, address)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_contract_interactions
(
    `chain_id` UInt32 DEFAULT 480 CODEC(ZSTD(1)),
    `from_address` FixedString(42) CODEC(ZSTD(3)),
    `to_address` FixedString(42) CODEC(ZSTD(3)),
    `function_selector` FixedString(10) CODEC(ZSTD(3)),
    `call_count` UInt64 CODEC(Delta(8), ZSTD(3)),
    `total_value_wei` UInt256 CODEC(ZSTD(3)),
    `error_count` UInt64 CODEC(Delta(8), ZSTD(3)),
    `last_seen_block` SimpleAggregateFunction(max, UInt64) CODEC(Delta(8), ZSTD(3)),
    `updated_at` SimpleAggregateFunction(max, DateTime) DEFAULT now() CODEC(DoubleDelta, ZSTD(3)),
    INDEX idx_to_address to_address TYPE bloom_filter(0.01) GRANULARITY 1,
    PROJECTION proj_by_to_address
    (
        SELECT *
        ORDER BY
            chain_id,
            to_address,
            from_address,
            function_selector
    ),
    PROJECTION proj_from_address_agg
    (
        SELECT
            from_address,
            to_address,
            sum(call_count) AS cnt
        GROUP BY
            chain_id,
            from_address,
            to_address
    )
)
ENGINE = SummingMergeTree((call_count, error_count))
ORDER BY (chain_id, from_address, to_address, function_selector)
SETTINGS index_granularity = 8192, deduplicate_merge_projection_mode = 'rebuild';

CREATE TABLE IF NOT EXISTS worldchain.world_contract_functions
(
    `chain_id` UInt32 DEFAULT 480 CODEC(ZSTD(1)),
    `address` FixedString(42) CODEC(ZSTD(3)),
    `function_selector` FixedString(10) CODEC(ZSTD(3)),
    `function_name` String CODEC(ZSTD(6)),
    `signature` String CODEC(ZSTD(6)),
    `visibility` LowCardinality(String) CODEC(ZSTD(1)),
    `state_mutability` LowCardinality(String) CODEC(ZSTD(1)),
    `modifiers` Array(String) CODEC(ZSTD(3)),
    `internal_calls` Array(String) CODEC(ZSTD(3)),
    `external_calls` Array(FixedString(42)) CODEC(ZSTD(3)),
    `is_payable` UInt8 DEFAULT 0 CODEC(T64, ZSTD(1)),
    `requires_world_id` UInt8 DEFAULT 0 CODEC(T64, ZSTD(1)),
    `source_type` LowCardinality(String) CODEC(ZSTD(1)),
    `updated_at` DateTime DEFAULT now() CODEC(DoubleDelta, ZSTD(3))
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (chain_id, address, function_selector)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_mev_activity
(
    `chain_id` UInt32 DEFAULT 480 CODEC(ZSTD(1)),
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `tx_hash` FixedString(66) CODEC(ZSTD(3)),
    `tx_index` UInt32 CODEC(Delta(4), ZSTD(3)),
    `mev_type` LowCardinality(String) CODEC(ZSTD(1)),
    `extracted_value_wei` UInt256 CODEC(ZSTD(3)),
    `target_pools` Array(FixedString(42)) CODEC(ZSTD(3)),
    `flash_loan_provider` Nullable(FixedString(42)) CODEC(ZSTD(3)),
    `flash_loan_asset` Nullable(FixedString(42)) CODEC(ZSTD(3)),
    `flash_loan_amount` Nullable(UInt256) CODEC(ZSTD(3)),
    `sandwich_victim_tx` Nullable(FixedString(66)) CODEC(ZSTD(3)),
    `ingested_at` DateTime64(3) DEFAULT now64(3) CODEC(DoubleDelta, ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(ingested_at)
ORDER BY (chain_id, block_number, mev_type, tx_hash)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_mev_hourly_stats
(
    `timestamp_hour` DateTime CODEC(DoubleDelta, ZSTD(1)),
    `mev_type` LowCardinality(String) CODEC(ZSTD(1)),
    `total_events` SimpleAggregateFunction(sum, UInt64),
    `total_extracted_wei` SimpleAggregateFunction(sum, UInt256),
    `max_extracted_wei` SimpleAggregateFunction(max, UInt256)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(timestamp_hour)
ORDER BY (timestamp_hour, mev_type)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_trace_gaps
(
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `rpc_error_code` Int32 DEFAULT 0 CODEC(ZSTD(3)),
    `reason` String CODEC(ZSTD(3)),
    `attempted_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
    `resolved` UInt8 DEFAULT 0 CODEC(ZSTD(3)),
    `resolved_at` DateTime64(3) DEFAULT toDateTime64(0, 3) CODEC(DoubleDelta, ZSTD(3))
)
ENGINE = ReplacingMergeTree(attempted_at)
ORDER BY block_number
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.world_sequence_gaps
(
    `gap_lo` UInt64,
    `gap_hi` UInt64,
    `detected_at` DateTime64(3),
    `resolved` UInt8,
    `source` LowCardinality(String)
)
ENGINE = ReplacingMergeTree(detected_at)
ORDER BY (gap_lo, gap_hi)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.ingest_checkpoints
(
    `name` String,
    `last_committed_block` UInt64,
    `updated_at` DateTime64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY name
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.latest_oracle_prices
(
    `token_symbol` String,
    `price_usd` Float64,
    `updated_at` DateTime
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY token_symbol
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_keys
(
    `api_key` String,
    `client_name` String,
    `tier` LowCardinality(String),
    `requests_per_second` UInt32,
    `is_active` UInt8 DEFAULT 1,
    `created_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree
ORDER BY api_key
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_depeg_drift
(
    `token_address` FixedString(42) CODEC(ZSTD(3)),
    `pool_address` FixedString(42) CODEC(ZSTD(3)),
    `spot_price` Float64 CODEC(ZSTD(3)),
    `oracle_price` Float64 CODEC(ZSTD(3)),
    `drift_ratio` Float64 CODEC(ZSTD(3)),
    `pool_type` LowCardinality(String) CODEC(ZSTD(1)),
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `updated_at` DateTime CODEC(DoubleDelta, ZSTD(3))
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (token_address, updated_at)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_dex_swaps
(
    `ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
    `block_timestamp` DateTime CODEC(DoubleDelta, ZSTD(3)),
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
    `tick` Int32 CODEC(ZSTD(3)),
    INDEX idx_dex_swaps_block_timestamp block_timestamp TYPE minmax GRANULARITY 1
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(toDate(ingested_at))
ORDER BY (block_number, tx_hash, log_index)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_gas_by_function
(
    `address` FixedString(42) CODEC(ZSTD(3)),
    `function_selector` FixedString(10) CODEC(ZSTD(3)),
    `call_count` UInt64 CODEC(Delta(8), ZSTD(3)),
    `total_gas_used` UInt64 CODEC(Delta(8), ZSTD(3)),
    `avg_gas_used` SimpleAggregateFunction(any, Float64) CODEC(ZSTD(3)),
    `min_gas_used` SimpleAggregateFunction(min, UInt64) CODEC(Delta(8), ZSTD(3)),
    `max_gas_used` SimpleAggregateFunction(max, UInt64) CODEC(Delta(8), ZSTD(3)),
    `success_count` UInt64 CODEC(Delta(8), ZSTD(3)),
    `revert_count` UInt64 CODEC(Delta(8), ZSTD(3)),
    `updated_at` SimpleAggregateFunction(max, DateTime) DEFAULT now() CODEC(DoubleDelta, ZSTD(3))
)
ENGINE = SummingMergeTree((call_count, total_gas_used, success_count, revert_count))
ORDER BY (address, function_selector)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_morpho_activity
(
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `tx_hash` String CODEC(ZSTD(3)),
    `market_id` String CODEC(ZSTD(3)),
    `action_type` Enum8('Borrow' = 1, 'Repay' = 2, 'Liquidate' = 3) CODEC(ZSTD(3)),
    `user_address` String CODEC(ZSTD(3)),
    `assets_amount` Int256 CODEC(ZSTD(3)),
    `shares_amount` Int256 CODEC(ZSTD(3)),
    `ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3)),
    INDEX idx_morpho_time ingested_at TYPE minmax GRANULARITY 1
)
ENGINE = ReplacingMergeTree
ORDER BY (market_id, tx_hash, action_type)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_nft_transfers
(
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `contract_address` String CODEC(ZSTD(3)),
    `token_id` UInt256 CODEC(ZSTD(3)),
    `from_address` String CODEC(ZSTD(3)),
    `to_address` String CODEC(ZSTD(3)),
    `ingested_at` DateTime64(3) CODEC(Delta(8), ZSTD(3)),
    INDEX idx_nft_time ingested_at TYPE minmax GRANULARITY 1
)
ENGINE = ReplacingMergeTree
ORDER BY (contract_address, token_id, block_number)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS worldchain.api_token_balances
(
    `token_address` FixedString(42) CODEC(ZSTD(3)),
    `owner_address` FixedString(42) DEFAULT wallet_address CODEC(ZSTD(3)),
    `balance_change` Int256 CODEC(Delta(8), ZSTD(3)),
    `wallet_address` FixedString(42) CODEC(ZSTD(3)),
    INDEX idx_balances_owner owner_address TYPE bloom_filter(0.001) GRANULARITY 1,
    PROJECTION proj_by_wallet
    (
        SELECT *
        ORDER BY
            owner_address,
            token_address
    )
)
ENGINE = SummingMergeTree
ORDER BY (token_address, owner_address)
SETTINGS index_granularity = 8192, deduplicate_merge_projection_mode = 'rebuild';

CREATE TABLE IF NOT EXISTS worldchain.api_universal_defi_events
(
    `ingested_at` DateTime64(3) CODEC(DoubleDelta, ZSTD(3)),
    `block_timestamp` DateTime CODEC(DoubleDelta, ZSTD(3)),
    `block_number` UInt64 CODEC(Delta(8), ZSTD(3)),
    `tx_hash` FixedString(66) CODEC(ZSTD(3)),
    `log_index` UInt32 CODEC(ZSTD(3)),
    `contract_address` FixedString(42) CODEC(ZSTD(3)),
    `event_type` LowCardinality(String) CODEC(ZSTD(3)),
    `raw_data` String CODEC(ZSTD(3)),
    INDEX idx_defi_contract_address contract_address TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_defi_event_type event_type TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_defi_block_timestamp block_timestamp TYPE minmax GRANULARITY 1
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(toDate(ingested_at))
ORDER BY (block_number, tx_hash, log_index)
SETTINGS index_granularity = 8192;
