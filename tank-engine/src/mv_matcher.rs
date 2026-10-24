use crate::nodestream::Log;
use crate::types::*;

pub const TOPIC_TRANSFER: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
pub const TOPIC_V2_SWAP: &str = "0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822";
pub const TOPIC_V3_SWAP: &str = "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";
pub const TOPIC_V4_SWAP: &str = "0xcc267713d782161bc7855e903bc701fbfa1fbc8f5b4970425ccfb4e578c7eb46";
pub const TOPIC_GAS_SPONSOR: &str = "0x49628fd1471006c1482da88028e9ce4dbb080b815c9b0344d39e5a8e6ec1419f";
pub const TOPIC_WORLD_ID: &str = "0x4bd07c08a95610ec871ff21503c2ff5ddc0060fb8584dc8263ca7b52479e0a0a";
pub const TOPIC_ORACLE_UPDATE: &str = "0x0559884fd3a460db3073b7fc896cc77986f16e378210ded43186175bf646fc5f";
pub const TOPIC_ERC1155_SINGLE: &str = "0xc3d58168c5ae7397731d063d5bbf3d657854427243025f8372d0a8c7f2b5e7f6";
pub const TOPIC_ERC1155_BATCH: &str = "0x4a3900ab092e55739f6361d21624f950354f32db869e884178961c028217d898";
pub const TOPIC_AAVE_LIQUIDATE: &str = "0xe410921712a441631b098db8464619d08e5c127415a7700a8947602e1c9535c5";
pub const TOPIC_MORPHO_LIQUIDATE: &str = "0x298637213e89542a220268ecf4e1f74dd7303d8d80fbe05c5c1d19854728d706";
pub const TOPIC_UNISWAP_MINT: &str = "0x7a5308066211086c7a061be1945b1e1ab413143c7b749d21c1a93e8779b76c82";
pub const TOPIC_MORPHO_BORROW: &str = "0x600f862215c0e7a2b057d296681027b4e601556cb2f2e4a3621d960f4e3c9213";

pub struct ExtractedViews {
    pub transfer: Option<TokenTransferRow>,
    pub dex_swap: Option<DexSwapRow>,
    pub universal_event: Option<UniversalDefiEventRow>,
    pub dex_swap_detail: Option<ApiDexSwapDetailRow>,
    pub gas_sponsorship: Option<ApiGasSponsorshipRow>,
    pub humanity_feed: Option<ApiHumanityFeedRow>,
    pub nft_transfer: Option<ApiNftTransferRow>,
    pub oracle_price: Option<ApiOraclePriceRow>,
    pub erc1155_transfer: Option<WorldErc1155TransferRow>,
    pub uniswap_v3_price: Option<ApiUniswapV3PriceRow>,
    pub whale_alert: Option<ApiWhaleAlertRow>,
    pub lending_liquidation: Option<ApiLendingLiquidationRow>,
    pub token_balances: Vec<ApiTokenBalanceRow>,
    pub latest_oracle_price: Option<LatestOraclePriceRow>,
    pub uniswap_mint: Option<ApiUniswapMintRow>,
    pub morpho_activity: Option<ApiMorphoActivityRow>,
}

pub fn match_and_extract_views(
    log: &Log,
    block_number: u64,
    block_timestamp: u32,
    now_ms: u64,
    now_s: u32,
) -> ExtractedViews {
    let mut views = ExtractedViews {
        transfer: None,
        dex_swap: None,
        universal_event: None,
        dex_swap_detail: None,
        gas_sponsorship: None,
        humanity_feed: None,
        nft_transfer: None,
        oracle_price: None,
        erc1155_transfer: None,
        uniswap_v3_price: None,
        whale_alert: None,
        lending_liquidation: None,
        token_balances: Vec::new(),
        latest_oracle_price: None,
        uniswap_mint: None,
        morpho_activity: None,
    };

    if log.topics.is_empty() {
        return views;
    }

    let topic0 = log.topics[0].as_str();

    // 1. ERC-20 & ERC-721 Transfer Matcher + Token Balances
    if topic0.eq_ignore_ascii_case(TOPIC_TRANSFER) {
        if log.topics.len() >= 3 {
            let from_addr = str_to_fixed_42(&log.topics[1]);
            let to_addr = str_to_fixed_42(&log.topics[2]);
            let token_addr = str_to_fixed_42(&log.address);
            let (val, token_id) = if log.topics.len() == 3 {
                (parse_hex_value(&log.data), String::new())
            } else {
                (String::new(), parse_hex_value(&log.topics[3]))
            };

            views.transfer = Some(TokenTransferRow {
                block_number,
                tx_hash: str_to_fixed_66(&log.tx_hash),
                log_index: log.log_index,
                token_address: token_addr.clone(),
                from_address: from_addr.clone(),
                to_address: to_addr.clone(),
                value: val.clone(),
                token_id,
                ingested_at: now_ms,
                block_timestamp,
            });

            if log.topics.len() == 3 {
                let val_bytes = parse_u256_bytes(&log.data);
                views.token_balances.push(ApiTokenBalanceRow {
                    wallet_address: from_addr.clone(),
                    token_address: token_addr.clone(),
                    balance_change: val_bytes,
                });
                views.token_balances.push(ApiTokenBalanceRow {
                    wallet_address: to_addr,
                    token_address: token_addr,
                    balance_change: val_bytes,
                });
            }

            if let Ok(num) = val.parse::<u128>() {
                if num >= 100_000_000_000_000_000_000_000 {
                    views.whale_alert = Some(ApiWhaleAlertRow {
                        ingested_at: now_ms,
                        block_number,
                        tx_hash: str_to_fixed_66(&log.tx_hash),
                        log_index: log.log_index,
                        token_address: str_to_fixed_42(&log.address),
                        from_address: str_to_fixed_42(&log.topics[1]),
                        to_address: str_to_fixed_42(&log.topics[2]),
                        value_raw: val,
                    });
                }
            }
        }

        if log.topics.len() == 4 {
            views.nft_transfer = Some(ApiNftTransferRow {
                block_number,
                contract_address: log.address.to_lowercase(),
                token_id: parse_u256_bytes(&log.topics[3]),
                from_address: format!("0x{:0>40}", log.topics[1].trim_start_matches("0x").to_lowercase()),
                to_address: format!("0x{:0>40}", log.topics[2].trim_start_matches("0x").to_lowercase()),
                ingested_at: now_ms,
            });
        }
    }

    // 2. DEX Swap Matcher
    if topic0.eq_ignore_ascii_case(TOPIC_V2_SWAP) || topic0.eq_ignore_ascii_case(TOPIC_V3_SWAP) {
        views.dex_swap = Some(DexSwapRow {
            ingested_at: now_s,
            block_timestamp,
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            pool_address: str_to_fixed_42(&log.address),
            is_v2: if topic0.eq_ignore_ascii_case(TOPIC_V2_SWAP) { 1 } else { 0 },
            is_v3: if topic0.eq_ignore_ascii_case(TOPIC_V3_SWAP) { 1 } else { 0 },
            raw_data: log.data.clone(),
        });
    }

    // 3. Uniswap V3 Detailed Swap & Spot Price Matcher
    if topic0.eq_ignore_ascii_case(TOPIC_V3_SWAP) && log.topics.len() >= 3 && log.data.len() >= 258 {
        let data_clean = log.data.trim_start_matches("0x");
        let amount0_hex = if data_clean.len() >= 64 { &data_clean[0..64] } else { "0" };
        let amount1_hex = if data_clean.len() >= 128 { &data_clean[64..128] } else { "0" };
        let sqrt_price_hex = if data_clean.len() >= 192 { &data_clean[128..192] } else { "0" };
        let liquidity_hex = if data_clean.len() >= 256 { &data_clean[192..256] } else { "0" };
        let tick_hex = if data_clean.len() >= 320 { &data_clean[256..320] } else { "0" };
        let sqrt_p_bytes = parse_u256_hex_word_to_le(sqrt_price_hex);

        views.dex_swap_detail = Some(ApiDexSwapDetailRow {
            ingested_at: now_ms,
            block_timestamp,
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            log_index: log.log_index,
            pool_address: str_to_fixed_42(&log.address),
            sender: str_to_fixed_42(&log.topics[1]),
            recipient: str_to_fixed_42(&log.topics[2]),
            amount0: parse_u256_hex_word_to_le(amount0_hex),
            amount1: parse_u256_hex_word_to_le(amount1_hex),
            sqrt_price_x96: sqrt_p_bytes,
            liquidity: parse_u256_hex_word_to_le(liquidity_hex),
            tick: i32_from_hex_word(tick_hex),
        });

        views.uniswap_v3_price = Some(ApiUniswapV3PriceRow {
            block_timestamp,
            ingested_at: now_s,
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            pool_address: str_to_fixed_42(&log.address),
            sqrt_price_x96: sqrt_p_bytes,
            price_raw: 1.0,
        });
    }

    // 4. Gas Sponsorship Matcher
    if topic0.eq_ignore_ascii_case(TOPIC_GAS_SPONSOR) && log.topics.len() >= 4 {
        let data_clean = log.data.trim_start_matches("0x");
        let gas_cost_hex = if data_clean.len() >= 192 {
            data_clean[128..192].to_string()
        } else {
            String::new()
        };
        views.gas_sponsorship = Some(ApiGasSponsorshipRow {
            ingested_at: now_s,
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            human_address: str_to_fixed_42(&log.topics[2]),
            paymaster_address: str_to_fixed_42(&log.topics[3]),
            gas_cost_hex,
        });
    }

    // 5. Humanity Feed Matcher (Corrected 40 hex char padding)
    let contract_clean = log.address.trim_start_matches("0x").to_lowercase();
    if contract_clean == "00178354dd2595411ff79041f930e491a4df39a278"
        || contract_clean == "178354dd2595411ff79041f930e491a4df39a278"
    {
        if topic0.eq_ignore_ascii_case(TOPIC_WORLD_ID) && log.topics.len() >= 2 {
            views.humanity_feed = Some(ApiHumanityFeedRow {
                ingested_at: now_s,
                block_number,
                tx_hash: str_to_fixed_66(&log.tx_hash),
                human_address: str_to_fixed_42(&log.topics[1]),
                action_type: "WORLD_ID_VERIFIED".to_string(),
            });
        }
    }

    // 6. Universal DeFi Event Matcher
    let event_type = if contract_clean == "00178354dd2595411ff79041f930e491a4df39a278"
        || contract_clean == "178354dd2595411ff79041f930e491a4df39a278"
    {
        "WORLD_ID_VERIFICATION"
    } else if contract_clean == "4200000000000000000000000000000000000010" {
        "OFFICIAL_BRIDGE_TRANSFER"
    } else if topic0.eq_ignore_ascii_case(TOPIC_V2_SWAP) {
        "V2_SWAP"
    } else if topic0.eq_ignore_ascii_case(TOPIC_V3_SWAP) {
        "V3_SWAP"
    } else if topic0.eq_ignore_ascii_case(TOPIC_V4_SWAP) {
        "V4_SWAP"
    } else {
        ""
    };

    if !event_type.is_empty() {
        views.universal_event = Some(UniversalDefiEventRow {
            ingested_at: now_ms,
            block_timestamp,
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            log_index: log.log_index,
            contract_address: str_to_fixed_42(&log.address),
            event_type: event_type.to_string(),
            raw_data: log.data.clone(),
        });
    }

    // 7. Oracle Price Feeds Matcher & State Table Updates
    if topic0.eq_ignore_ascii_case(TOPIC_ORACLE_UPDATE) && log.topics.len() >= 2 {
        let sym = if contract_clean == "8bb2943ab030e3ee05a58d9832525b4f60a97fa0" {
            "WLD"
        } else if contract_clean == "e1d72a719171dceab9499757eb9d5aeb9e8d64a6" {
            "ETH"
        } else if contract_clean == "f4301686aff4ee36d70c718a9e62309b53862be8" {
            "USDC"
        } else {
            ""
        };

        if !sym.is_empty() {
            let price_hex = log.topics[1].trim_start_matches("0x");
            let price_usd = if let Ok(val) = u128::from_str_radix(price_hex, 16) {
                (val as f64) / 1e18
            } else {
                0.0
            };
            views.oracle_price = Some(ApiOraclePriceRow {
                block_timestamp,
                ingested_at: now_s,
                block_number,
                token_symbol: sym.to_string(),
                price_usd,
            });
            views.latest_oracle_price = Some(LatestOraclePriceRow {
                token_symbol: sym.to_string(),
                price_usd,
                updated_at: block_timestamp,
            });
        }
    }

    // 8. ERC-1155 TransferSingle & TransferBatch Matcher
    if (topic0.eq_ignore_ascii_case(TOPIC_ERC1155_SINGLE) || topic0.eq_ignore_ascii_case(TOPIC_ERC1155_BATCH))
        && log.topics.len() >= 4
    {
        let data_clean = log.data.trim_start_matches("0x");
        let token_id_hex = if data_clean.len() >= 64 { &data_clean[0..64] } else { "0" };
        let value_hex = if data_clean.len() >= 128 { &data_clean[64..128] } else { "0" };

        views.erc1155_transfer = Some(WorldErc1155TransferRow {
            chain_id: 480,
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            log_index: log.log_index,
            token_address: str_to_fixed_42(&log.address),
            operator: str_to_fixed_42(&log.topics[1]),
            from_address: str_to_fixed_42(&log.topics[2]),
            to_address: str_to_fixed_42(&log.topics[3]),
            token_id: parse_u256_bytes(token_id_hex),
            value: parse_u256_bytes(value_hex),
            is_batch: if topic0.eq_ignore_ascii_case(TOPIC_ERC1155_BATCH) { 1 } else { 0 },
            is_canonical: 1,
            ingested_at: now_ms,
        });
    }

    // 9. Lending Protocol Liquidation Matcher (Aave v3 + Morpho)
    if (topic0.eq_ignore_ascii_case(TOPIC_AAVE_LIQUIDATE) || topic0.eq_ignore_ascii_case(TOPIC_MORPHO_LIQUIDATE))
        && log.topics.len() >= 3
    {
        views.lending_liquidation = Some(ApiLendingLiquidationRow {
            block_number,
            tx_hash: log.tx_hash.clone(),
            liquidator: log.topics[1].clone(),
            borrower: log.topics[2].clone(),
            collateral_asset: log.address.clone(),
            debt_asset: String::new(),
            debt_covered: parse_u256_bytes(&log.data),
            ingested_at: now_ms,
        });
    }

    // 10. Uniswap Mint Events
    if topic0.eq_ignore_ascii_case(TOPIC_UNISWAP_MINT) && log.topics.len() >= 3 {
        views.uniswap_mint = Some(ApiUniswapMintRow {
            block_number,
            tx_hash: str_to_fixed_66(&log.tx_hash),
            pool_address: str_to_fixed_42(&log.address),
            sender: str_to_fixed_42(&log.topics[1]),
            owner: str_to_fixed_42(&log.topics[2]),
            amount: parse_u256_bytes(&log.data),
            ingested_at: now_ms,
        });
    }

    // 11. Morpho Borrow & Activity Events
    if topic0.eq_ignore_ascii_case(TOPIC_MORPHO_BORROW) && log.topics.len() >= 3 {
        views.morpho_activity = Some(ApiMorphoActivityRow {
            block_number,
            tx_hash: log.tx_hash.clone(),
            market_id: log.topics[1].clone(),
            action_type: 1, // 1 = Borrow
            user_address: log.topics[2].clone(),
            assets_amount: parse_u256_bytes(&log.data),
            shares_amount: [0u8; 32],
            ingested_at: now_ms,
        });
    }

    views
}

fn parse_hex_value(s: &str) -> String {
    let clean = s.trim_start_matches("0x");
    if clean.is_empty() {
        return "0".to_string();
    }
    match u128::from_str_radix(clean, 16) {
        Ok(v) => v.to_string(),
        Err(_) => "0".to_string(),
    }
}
