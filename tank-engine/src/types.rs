use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedString66(pub [u8; 66]);

impl serde::Serialize for FixedString66 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(66)?;
        for b in &self.0 {
            tup.serialize_element(b)?;
        }
        tup.end()
    }
}

impl<'de> serde::Deserialize<'de> for FixedString66 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = FixedString66;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a 66-byte array")
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut buf = [0u8; 66];
                for i in 0..66 {
                    if let Some(b) = seq.next_element()? {
                        buf[i] = b;
                    }
                }
                Ok(FixedString66(buf))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut buf = [0u8; 66];
                let len = v.len().min(66);
                buf[..len].copy_from_slice(&v[..len]);
                Ok(FixedString66(buf))
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut buf = [0u8; 66];
                let bytes = v.as_bytes();
                let len = bytes.len().min(66);
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(FixedString66(buf))
            }
        }
        deserializer.deserialize_tuple(66, Visitor)
    }
}

pub fn str_to_fixed_66(s: &str) -> FixedString66 {
    let mut buf = [0u8; 66];
    let clean = s.trim_start_matches("0x");
    let formatted = if clean.len() >= 64 {
        let start = clean.len() - 64;
        format!("0x{}", clean[start..].to_lowercase())
    } else {
        format!("0x{:0>64}", clean.to_lowercase())
    };
    let bytes = formatted.as_bytes();
    let len = bytes.len().min(66);
    buf[..len].copy_from_slice(&bytes[..len]);
    FixedString66(buf)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedString42(pub [u8; 42]);

impl serde::Serialize for FixedString42 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(42)?;
        for b in &self.0 {
            tup.serialize_element(b)?;
        }
        tup.end()
    }
}

impl<'de> serde::Deserialize<'de> for FixedString42 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = FixedString42;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a 42-byte array")
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut buf = [0u8; 42];
                for i in 0..42 {
                    if let Some(b) = seq.next_element()? {
                        buf[i] = b;
                    }
                }
                Ok(FixedString42(buf))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut buf = [0u8; 42];
                let len = v.len().min(42);
                buf[..len].copy_from_slice(&v[..len]);
                Ok(FixedString42(buf))
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut buf = [0u8; 42];
                let bytes = v.as_bytes();
                let len = bytes.len().min(42);
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(FixedString42(buf))
            }
        }
        deserializer.deserialize_tuple(42, Visitor)
    }
}

pub fn str_to_fixed_42(s: &str) -> FixedString42 {
    let mut buf = [0u8; 42];
    let clean = s.trim_start_matches("0x");
    let formatted = if clean.len() >= 40 {
        let start = clean.len() - 40;
        format!("0x{}", clean[start..].to_lowercase())
    } else {
        format!("0x{:0>40}", clean.to_lowercase())
    };
    let bytes = formatted.as_bytes();
    let len = bytes.len().min(42);
    buf[..len].copy_from_slice(&bytes[..len]);
    FixedString42(buf)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedString10(pub [u8; 10]);

impl serde::Serialize for FixedString10 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(10)?;
        for b in &self.0 {
            tup.serialize_element(b)?;
        }
        tup.end()
    }
}

impl<'de> serde::Deserialize<'de> for FixedString10 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = FixedString10;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a 10-byte array")
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut buf = [0u8; 10];
                for i in 0..10 {
                    if let Some(b) = seq.next_element()? {
                        buf[i] = b;
                    }
                }
                Ok(FixedString10(buf))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut buf = [0u8; 10];
                let len = v.len().min(10);
                buf[..len].copy_from_slice(&v[..len]);
                Ok(FixedString10(buf))
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut buf = [0u8; 10];
                let bytes = v.as_bytes();
                let len = bytes.len().min(10);
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(FixedString10(buf))
            }
        }
        deserializer.deserialize_tuple(10, Visitor)
    }
}

pub fn str_to_fixed_10(s: &str) -> FixedString10 {
    let mut buf = [0u8; 10];
    let clean = s.trim_start_matches("0x");
    let formatted = if clean.len() >= 8 {
        format!("0x{}", &clean[..8].to_lowercase())
    } else {
        format!("0x{:0<8}", clean.to_lowercase())
    };
    let bytes = formatted.as_bytes();
    let len = bytes.len().min(10);
    buf[..len].copy_from_slice(&bytes[..len]);
    FixedString10(buf)
}

pub fn parse_u256_bytes(s: &str) -> [u8; 32] {
    // HEX 32-byte word parser (log-data words; two's complement preserved for Int256 cols).
    // Restored to original hex-radix semantics 2026-09-09 (callers pass hex words).
    let mut bytes = [0u8; 32];
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if t.is_empty() || t.len() > 64 {
        return bytes;
    }
    if let Ok(v) = u128::from_str_radix(t, 16) {
        bytes[..16].copy_from_slice(&v.to_le_bytes());
        return bytes;
    }
    // wide hex: big-endian nibble accumulation into LE limbs
    let mut limbs = [0u64; 4];
    for ch in t.bytes() {
        let d = match (ch as char).to_digit(16) {
            Some(v) => v as u64,
            None => return bytes,
        };
        let mut carry = d;
        for limb in limbs.iter_mut() {
            let prod = (*limb as u128) * 16 + carry as u128;
            *limb = prod as u64;
            carry = (prod >> 64) as u64;
        }
    }
    for (i, limb) in limbs.iter().enumerate() {
        bytes[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_le_bytes());
    }
    bytes
}

pub fn parse_u256_dec_bytes(s: &str) -> [u8; 32] {
    // DECIMAL string parser (exex value strings; radix-explicit — FINDING B fix).
    let mut bytes = [0u8; 32];
    let t = s.trim();
    if t.is_empty() {
        return bytes;
    }
    let mut limbs = [0u64; 4];
    for ch in t.bytes() {
        if !ch.is_ascii_digit() {
            return bytes; // invalid -> zeros
        }
        let mut carry = (ch - b'0') as u64;
        for limb in limbs.iter_mut() {
            let prod = (*limb as u128) * 10 + carry as u128;
            *limb = prod as u64;
            carry = (prod >> 64) as u64;
        }
    }
    for (i, limb) in limbs.iter().enumerate() {
        bytes[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_le_bytes());
    }
    bytes
}

pub fn parse_u256_hex_word_to_le(s: &str) -> [u8; 32] {
    // 64-char (or shorter, right-aligned) hex word -> 32-byte LITTLE-ENDIAN array.
    // Decodes hex pairs properly (hi*16+lo per byte), right-aligned, then reverses
    // BE->LE to match the column byte order. Two's complement preserved for Int256.
    let mut be = [0u8; 32];
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    let t = if t.len() > 64 { &t[t.len() - 64..] } else { t };
    let b = t.as_bytes();
    let mut i = b.len();
    let mut pos = 32usize;
    while i > 0 && pos > 0 {
        let lo = (b[i - 1] as char).to_digit(16).unwrap_or(0) as u8;
        pos -= 1;
        if i >= 2 {
            let hi = (b[i - 2] as char).to_digit(16).unwrap_or(0) as u8;
            be[pos] = hi * 16 + lo;
            i -= 2;
        } else {
            be[pos] = lo;
            i -= 1;
        }
    }
    be.reverse();
    be
}

pub fn i32_from_hex_word(word: &str) -> i32 {
    // low 32 bits of a sign-extended 256-bit word == the int32 value
    let t = word.trim().trim_start_matches("0x");
    let w64 = if t.len() >= 64 { &t[t.len() - 64..] } else { t };
    match i64::from_str_radix(w64, 16) {
        Ok(v) => {
            let sh = v - (v >> 31 << 32); // sign-extend from low 32 for narrow words
            (sh & 0xFFFF_FFFF) as u32 as i32
        }
        Err(_) => {
            let mut v: i64 = 0;
            for ch in w64.bytes() {
                v = v.wrapping_mul(16).wrapping_add((ch as char).to_digit(16).unwrap_or(0) as i64);
            }
            v as i32
        }
    }
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct BlockRow {
    pub chain_id: u32,
    pub block_number: u64,
    pub block_hash: FixedString66,
    pub parent_hash: FixedString66,
    pub timestamp: u64,
    pub miner: FixedString42,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub base_fee_per_gas: u64,
    pub transactions_count: u32,
    pub size_bytes: u32,
    pub ingested_at: u64,
    pub is_canonical: u8,
    pub healed_by: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct TransactionRow {
    pub chain_id: u32,
    pub block_number: u64,
    pub block_hash: FixedString66,
    pub tx_index: u32,
    pub tx_hash: FixedString66,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub value_wei: [u8; 32],
    pub mint_wei: [u8; 32],
    pub gas_price_wei: u64,
    pub max_fee_per_gas_wei: u64,
    pub max_priority_fee_wei: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub nonce: u64,
    pub tx_type: u8,
    pub is_system_tx: u8,
    pub input_data: String,
    pub status: u8,
    pub effective_gas_price_wei: u64,
    pub ingested_at: u64,
    pub block_timestamp: u32,
    pub l1_fee_wei: [u8; 32],
    pub l1_gas_price_wei: u64,
    pub is_canonical: u8,
    pub healed_by: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize, Deserialize)]
pub struct LogRow {
    pub chain_id: u32,
    pub block_number: u64,
    pub block_hash: FixedString66,
    pub tx_hash: FixedString66,
    pub tx_index: u32,
    pub log_index: u32,
    pub address: FixedString42,
    pub data: String,
    pub topics: Vec<FixedString66>,
    pub ingested_at: u64,
    pub block_timestamp: u32,
    pub is_canonical: u8,
    pub healed_by: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct TraceRow {
    pub chain_id: u32,
    pub block_number: u64,
    pub block_hash: FixedString66,
    pub tx_index: u32,
    pub tx_hash: FixedString66,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub value_wei: [u8; 32],
    pub input_data: String,
    pub output_data: String,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub trace_type: String,
    pub call_type: String,
    pub error: String,
    pub depth: u32,
    pub ingested_at: u64,
    pub block_timestamp: u32,
    pub is_canonical: u8,
    pub trace_index: u32,
    pub healed_by: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct TokenTransferRow {
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub log_index: u32,
    pub token_address: FixedString42,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub value: String,
    pub token_id: String,
    pub ingested_at: u64,
    pub block_timestamp: u32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct DexSwapRow {
    pub ingested_at: u32,
    pub block_timestamp: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub pool_address: FixedString42,
    pub is_v2: u8,
    pub is_v3: u8,
    pub raw_data: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct UniversalDefiEventRow {
    pub ingested_at: u64,
    pub block_timestamp: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub log_index: u32,
    pub contract_address: FixedString42,
    pub event_type: String,
    pub raw_data: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiDexSwapDetailRow {
    pub ingested_at: u64,
    pub block_timestamp: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub log_index: u32,
    pub pool_address: FixedString42,
    pub sender: FixedString42,
    pub recipient: FixedString42,
    pub amount0: [u8; 32],
    pub amount1: [u8; 32],
    #[serde(rename = "sqrtPriceX96")]
    pub sqrt_price_x96: [u8; 32],
    pub liquidity: [u8; 32],
    pub tick: i32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiGasSponsorshipRow {
    pub ingested_at: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub human_address: FixedString42,
    pub paymaster_address: FixedString42,
    pub gas_cost_hex: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiHumanityFeedRow {
    pub ingested_at: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub human_address: FixedString42,
    pub action_type: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiNftTransferRow {
    pub block_number: u64,
    pub contract_address: String,
    pub token_id: [u8; 32],
    pub from_address: String,
    pub to_address: String,
    pub ingested_at: u64,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiOraclePriceRow {
    pub block_timestamp: u32,
    pub ingested_at: u32,
    pub block_number: u64,
    pub token_symbol: String,
    pub price_usd: f64,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct WorldErc1155TransferRow {
    pub chain_id: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub log_index: u32,
    pub token_address: FixedString42,
    pub operator: FixedString42,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub token_id: [u8; 32],
    pub value: [u8; 32],
    pub is_batch: u8,
    pub is_canonical: u8,
    pub ingested_at: u64,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiInternalTransferRow {
    pub ingested_at: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub value_wei: [u8; 32],
    pub depth: u32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiUniswapV3PriceRow {
    pub block_timestamp: u32,
    pub ingested_at: u32,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub pool_address: FixedString42,
    pub sqrt_price_x96: [u8; 32],
    pub price_raw: f64,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiWhaleAlertRow {
    pub ingested_at: u64,
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub log_index: u32,
    pub token_address: FixedString42,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub value_raw: String,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct WorldContractInteractionRow {
    pub chain_id: u32,
    pub from_address: FixedString42,
    pub to_address: FixedString42,
    pub function_selector: FixedString10,
    pub call_count: u64,
    pub total_value_wei: [u8; 32],
    pub error_count: u64,
    pub last_seen_block: u64,
    pub updated_at: u32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiDepegDriftRow {
    pub token_address: FixedString42,
    pub pool_address: FixedString42,
    pub spot_price: f64,
    pub oracle_price: f64,
    pub drift_ratio: f64,
    pub pool_type: String,
    pub block_number: u64,
    pub updated_at: u32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiGasByFunctionRow {
    pub address: FixedString42,
    pub function_selector: FixedString10,
    pub call_count: u64,
    pub total_gas_used: u64,
    pub avg_gas_used: f64,
    pub min_gas_used: u64,
    pub max_gas_used: u64,
    pub success_count: u64,
    pub revert_count: u64,
    pub updated_at: u32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiLendingLiquidationRow {
    pub block_number: u64,
    pub tx_hash: String,
    pub liquidator: String,
    pub borrower: String,
    pub collateral_asset: String,
    pub debt_asset: String,
    pub debt_covered: [u8; 32],
    pub ingested_at: u64,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct WorldMevHourlyStatsRow {
    pub timestamp_hour: u32,
    pub mev_type: String,
    pub total_events: u64,
    pub total_extracted_wei: [u8; 32],
    pub max_extracted_wei: [u8; 32],
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiTokenBalanceRow {
    pub wallet_address: FixedString42,
    pub token_address: FixedString42,
    pub balance_change: [u8; 32],
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct LatestOraclePriceRow {
    pub token_symbol: String,
    pub price_usd: f64,
    pub updated_at: u32,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiUniswapMintRow {
    pub block_number: u64,
    pub tx_hash: FixedString66,
    pub pool_address: FixedString42,
    pub sender: FixedString42,
    pub owner: FixedString42,
    pub amount: [u8; 32],
    pub ingested_at: u64,
}

#[derive(Debug, Clone, clickhouse::Row, Serialize)]
pub struct ApiMorphoActivityRow {
    pub block_number: u64,
    pub tx_hash: String,
    pub market_id: String,
    pub action_type: u8,
    pub user_address: String,
    pub assets_amount: [u8; 32],
    pub shares_amount: [u8; 32],
    pub ingested_at: u64,
}
