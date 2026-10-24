-- 1. Unified token_balances_v2
CREATE TABLE IF NOT EXISTS worldchain.token_balances_v2
(
    `token_address` FixedString(42) CODEC(ZSTD(3)),
    `owner_address` FixedString(42) CODEC(ZSTD(3)),
    `balance_change` Int256 CODEC(Delta(8), ZSTD(3)),

    INDEX idx_balances_owner owner_address TYPE bloom_filter(0.001) GRANULARITY 1,

    PROJECTION proj_by_wallet
    (
        SELECT *
        ORDER BY owner_address, token_address
    )
)
ENGINE = SummingMergeTree
ORDER BY (token_address, owner_address)
SETTINGS index_granularity = 8192, deduplicate_merge_projection_mode = 'rebuild';

-- 2. Hardened world_transactions_v2
CREATE TABLE IF NOT EXISTS worldchain.world_transactions_v2
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
TTL toDateTime(block_timestamp) + toIntervalDay(180) TO VOLUME 'cold'
SETTINGS index_granularity = 8192, storage_policy = 'hot_and_cold';

-- 3. Hardened world_logs_v2
CREATE TABLE IF NOT EXISTS worldchain.world_logs_v2
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
        ORDER BY topics[1], block_number
    )
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(block_timestamp)
ORDER BY (chain_id, block_number, tx_index, log_index)
TTL toDateTime(block_timestamp) + toIntervalDay(180) TO VOLUME 'cold'
SETTINGS index_granularity = 8192, parts_to_throw_insert = 3000, max_parts_in_total = 100000, storage_policy = 'hot_and_cold', deduplicate_merge_projection_mode = 'rebuild';

-- 4. Hardened world_traces_v2
CREATE TABLE IF NOT EXISTS worldchain.world_traces_v2
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
TTL toDateTime(block_timestamp) + toIntervalDay(180) TO VOLUME 'cold'
SETTINGS index_granularity = 8192, storage_policy = 'hot_and_cold';

-- 5. Hardened world_token_transfers_v2
CREATE TABLE IF NOT EXISTS worldchain.world_token_transfers_v2
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
        ORDER BY from_address, block_number
    ),
    PROJECTION proj_by_to
    (
        SELECT *
        ORDER BY to_address, block_number
    )
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(block_timestamp)
ORDER BY (block_number, tx_hash, log_index)
TTL toDateTime(block_timestamp) + toIntervalDay(180) TO VOLUME 'cold'
SETTINGS index_granularity = 8192, storage_policy = 'hot_and_cold', deduplicate_merge_projection_mode = 'rebuild';
