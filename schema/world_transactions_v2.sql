CREATE TABLE worldchain.world_transactions_v2
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
    `l1_fee_wei` UInt256 CODEC(ZSTD(3)),
    `l1_gas_price_wei` UInt64 CODEC(ZSTD(3)),
    `is_canonical` UInt8 DEFAULT 1 CODEC(ZSTD(3)),
    `healed_by` LowCardinality(String) DEFAULT '' CODEC(ZSTD(3)),
    
    INDEX idx_from_address from_address TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_to_address to_address TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(ingested_at)
ORDER BY (chain_id, block_number, tx_index, tx_hash)
SETTINGS index_granularity = 8192, parts_to_delay_insert = 300, parts_to_throw_insert = 600, max_parts_in_total = 200000;
