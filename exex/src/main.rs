use eyre::{eyre, Result};
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Semaphore};
use tokio_stream::wrappers::{BroadcastStream, UnixListenerStream};
use tonic::{transport::Server, Request, Response, Status};
use reth::api::FullNodeComponents;
use reth_exex::{ExExContext, ExExEvent, ExExNotificationsStream};
use reth_exex_types::ExExHead;
use reth_optimism_cli::Cli;
use reth_optimism_node::node::OpNode;
use reth_optimism_primitives::OpReceipt;
use reth_primitives_traits::{AlloyBlockHeader, BlockBody, InMemorySize};
use reth_primitives_traits::transaction::TxHashRef;
use reth_primitives::NodePrimitives;
use reth_primitives_traits::SignedTransaction;
use alloy_consensus::{transaction::Recovered, Transaction, TxReceipt};
use alloy_eips::{
    eip2718::{Encodable2718, Typed2718},
    BlockNumHash,
};
use alloy_primitives::U256;
use reth_provider::{BlockHashReader, HeaderProvider, ReceiptProvider, StateProviderFactory, StageCheckpointReader};
use op_alloy_flz::flz_compress_len;
use reth_revm::database::StateProviderDatabase;
use reth_revm::db::State;
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};
use reth_evm::{ConfigureEvm, EvmEnvFor, TxEnvFor};


pub mod nodestream {
    tonic::include_proto!("nodestream");
}

use nodestream::node_stream_server::{NodeStream, NodeStreamServer};
use nodestream::{BlockNotification, Log, SubscribeRequest, Trace, Transaction as GrpcTransaction};

const NODESTREAM_SOCK: &str = "/var/run/worldchain/node_stream.sock";
/// FINAL PLAN 2026-08-22: shrink ExEx→tank broadcast (was 1_048_576).
const BROADCAST_CAP: usize = 512;
/// In-flight collect_block_traces tasks. Overflow → skip traces, tank F5 gap/heal.
const TRACE_QUEUE_CAP: usize = 4;
/// MDBX RO window: reopen history_by_block_hash every N txs (no whole-block long RO).
const TRACE_RO_CHUNK: usize = 16;

type BoxStream<T> = Pin<Box<dyn Stream<Item = T> + Send + 'static>>;

struct NodeStreamSvc {
    tx: broadcast::Sender<BlockNotification>,
}

#[tonic::async_trait]
impl NodeStream for NodeStreamSvc {
    type SubscribeStream = BoxStream<Result<BlockNotification, Status>>;

    async fn subscribe(
        &self,
        req: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let start = req.into_inner().start_block;
        let rx = self.tx.subscribe();
        let stream = BroadcastStream::new(rx).filter_map(move |item| {
            futures::future::ready(match item {
                Ok(n) if start == 0 || n.number >= start => Some(Ok(n)),
                _ => None,
            })
        });
        Ok(Response::new(Box::pin(stream) as Self::SubscribeStream))
    }
}

// Fjord/Isthmus L1 fee via compressed-size estimate
fn fjord_fee_from_encoded(
    encoded: &[u8],
    l1_base_fee: U256,
    l1_blob_fee: U256,
    base_fee_scalar: u32,
    blob_fee_scalar: u32,
) -> U256 {
    let fastlz = flz_compress_len(encoded) as i128;
    let mut est = -42_585_600_i128 + 836_500 * fastlz;
    if est < 100_000_000 {
        est = 100_000_000;
    }
    let fee_sum = (l1_base_fee * U256::from(base_fee_scalar) * U256::from(16u64))
        + (l1_blob_fee * U256::from(blob_fee_scalar));
    (U256::from(est as u64) * fee_sum) / U256::from(1_000_000_000_000u64)
}

// Strong per-tx validator: checks count, monotonic cumulative gas, non-zero gas delta
// for every non-system tx, and final cumgas == block header gas_used.
fn receipts_match_block<T: Transaction>(
    block_number: u64,
    block_gas_used: u64,
    txs: &[T],
    receipts: &[OpReceipt],
    source: &str,
) -> bool {
    if receipts.len() != txs.len() {
        eprintln!(
            "[worldchain-exex] reject_receipts block={} source={} reason=count_mismatch txs={} receipts={}",
            block_number, source, txs.len(), receipts.len()
        );
        return false;
    }
    if txs.is_empty() {
        return block_gas_used == 0;
    }
    let mut prev_cum = 0u64;
    for (idx, (tx, r)) in txs.iter().zip(receipts.iter()).enumerate() {
        let cum = r.cumulative_gas_used();
        if cum < prev_cum {
            eprintln!(
                "[worldchain-exex] reject_receipts block={} source={} tx_index={} reason=non_monotonic prev={} curr={}",
                block_number, source, idx, prev_cum, cum
            );
            return false;
        }
        let delta = cum - prev_cum;
        if tx.ty() != 126 && delta == 0 {
            eprintln!(
                "[worldchain-exex] reject_receipts block={} source={} tx_index={} tx_type={} reason=zero_delta_non_system",
                block_number, source, idx, tx.ty()
            );
            return false;
        }
        prev_cum = cum;
    }
    if prev_cum != block_gas_used {
        eprintln!(
            "[worldchain-exex] reject_receipts block={} source={} reason=final_cum_mismatch header={} final={}",
            block_number, source, block_gas_used, prev_cum
        );
        return false;
    }
    true
}

// ClickHouse checkpoint via native clickhouse-client subprocess.
// No HTTP port, no reqwest, no OpenSSL.
async fn ch_last_indexed_block() -> u64 {
    let out = tokio::process::Command::new("clickhouse-client")
        .args(["--query", "SELECT max(block_number) FROM worldchain.world_blocks"])
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let v = s.trim().parse::<u64>().unwrap_or(0);
            eprintln!("[worldchain-exex] ch_last_indexed_block={}", v);
            v
        }
        Ok(o) => {
            eprintln!(
                "[worldchain-exex] clickhouse-client stderr: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            0
        }
        Err(e) => {
            eprintln!("[worldchain-exex] clickhouse-client exec error: {}", e);
            0
        }
    }
}

async fn init_nodestream_exex<Node>(
    mut ctx: ExExContext<Node>,
) -> Result<impl std::future::Future<Output = Result<()>> + Send + 'static>
where
    Node: FullNodeComponents,
    Node::Types: reth::api::NodeTypes,
    <Node::Types as reth::api::NodeTypes>::Primitives: NodePrimitives<
        SignedTx: SignedTransaction + Typed2718 + Transaction + Encodable2718,
        Receipt = OpReceipt,
    >,
    Node::Provider: StateProviderFactory
        + ReceiptProvider
        + BlockHashReader
        + HeaderProvider
        + Clone
        + Unpin
        + Send
        + 'static,
    Node::Evm: ConfigureEvm<
            Primitives = <Node::Types as reth::api::NodeTypes>::Primitives,
        > + Clone
        + Send
        + Sync
        + 'static,
{
    let mut exex_head_number = ch_last_indexed_block().await;

    let mut exex_head_hash = None;
    while exex_head_number > 0 {
        if let Some(hash) = ctx.provider().block_hash(exex_head_number)? {
            exex_head_hash = Some(hash);
            break;
        }
        exex_head_number -= 1;
    }
    let exex_head_hash = match exex_head_hash {
        Some(h) => h,
        None => ctx.provider().block_hash(0)?.unwrap_or_default(),
    };

    let bypass_limit = std::env::var("EXEX_BYPASS_UNDER_BLOCK")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    eprintln!(
        "[worldchain-exex] startup: exex_head={} hash=0x{} bypass_limit={}",
        exex_head_number,
        hex::encode(exex_head_hash),
        bypass_limit
    );

    ctx.notifications.set_with_head(ExExHead {
        block: BlockNumHash {
            number: exex_head_number,
            hash: exex_head_hash,
        },
    });

    let _ = tokio::fs::remove_file(NODESTREAM_SOCK).await;
    let (tx_ch, _) = broadcast::channel::<BlockNotification>(BROADCAST_CAP);
    let trace_slots = Arc::new(Semaphore::new(TRACE_QUEUE_CAP));
    eprintln!(
        "[worldchain-exex] broadcast_cap={} trace_queue={} trace_ro_chunk={}",
        BROADCAST_CAP, TRACE_QUEUE_CAP, TRACE_RO_CHUNK
    );
    let svc = NodeStreamSvc { tx: tx_ch.clone() };
    let srv = Server::builder()
        .http2_keepalive_interval(Some(std::time::Duration::from_secs(30)))
        .http2_keepalive_timeout(Some(std::time::Duration::from_secs(120)))
        .add_service(NodeStreamServer::new(svc))
        .serve_with_incoming(UnixListenerStream::new(UnixListener::bind(NODESTREAM_SOCK)?));
    tokio::spawn(async move { srv.await.ok() });

    // Phase-2 F2: ExEx future must NEVER complete (consensus fatal if it does).
    // Notification stream errors / EOF → log + continue / park — never `?` out to Ok(()).
    Ok(async move {
        loop {
        while let Some(notification) = ctx.notifications.next().await {
            let notification = match notification {
                Ok(n) => n,
                Err(e) => {
                    eprintln!(
                        "[worldchain-exex] notification error (non-fatal, continuing): {:?}",
                        e
                    );
                    continue;
                }
            };
            if let Some(committed_chain) = notification.committed_chain() {
                for (block, exex_receipts) in committed_chain.blocks_and_receipts() {
                    let hdr = block.header();
                    if hdr.number() < bypass_limit {
                        ctx.events
                            .send(ExExEvent::FinishedHeight(alloy_eips::BlockNumHash {
                                number: hdr.number(),
                                hash: block.hash(),
                            }))
                            .ok();
                        continue;
                    }
                    let txs_in_block: Vec<_> = block.body().transactions().into_iter().cloned().collect();
                    let senders_in_block: Vec<_> = block.senders().to_vec();
                    let expected = txs_in_block.len();

                    let exex_ok = receipts_match_block(
                        hdr.number(), hdr.gas_used(), &txs_in_block, exex_receipts, "notification",
                    );
                    eprintln!(
                        "[worldchain-exex] block_seen number={} txs={} exex_receipts={} exex_receipts_ok={}",
                        hdr.number(), expected, exex_receipts.len(), exex_ok
                    );

                    let mut local_receipts: Vec<OpReceipt> = if exex_ok {
                        exex_receipts.to_vec()
                    } else {
                        Vec::new()
                    };

                    // Fall back to DB receipts if ExEx receipts are bad
                    if local_receipts.is_empty() && expected > 0 {
                        loop {
                            if let Ok(Some(db_receipts)) =
                                ctx.provider().receipts_by_block(block.hash().into())
                            {
                                if receipts_match_block(
                                    hdr.number(), hdr.gas_used(), &txs_in_block, &db_receipts, "db",
                                ) {
                                    eprintln!(
                                        "[worldchain-exex] receipts_ready block={} source=db",
                                        hdr.number()
                                    );
                                    local_receipts = db_receipts;
                                    break;
                                }
                            }
                            eprintln!(
                                "[worldchain-exex] wait_receipts block={} source=db",
                                hdr.number()
                            );
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }

                    if !receipts_match_block(
                        hdr.number(), hdr.gas_used(), &txs_in_block, &local_receipts, "final",
                    ) && expected > 0 {
                        // Phase-2 F2: still advance FinishedHeight so consensus does not stall;
                        // tank will see a hole and enqueue raw heal (F5) rather than freezing ExEx.
                        eprintln!(
                            "[worldchain-exex] skip block={} hash=0x{} receipts_invalid — emitting FinishedHeight",
                            hdr.number(), hex::encode(block.hash())
                        );
                        ctx.events
                            .send(ExExEvent::FinishedHeight(alloy_eips::BlockNumHash {
                                number: hdr.number(),
                                hash: block.hash(),
                            }))
                            .ok();
                        continue;
                    }

                    // Parse OP L1 info tx (type 0x7e) for L1 fee scalars
                    let mut base_fee_scalar = 0u32;
                    let mut blob_fee_scalar = 0u32;
                    let mut is_fjord = false;
                    let mut l1_base_fee = U256::ZERO;
                    let mut l1_blob_fee = U256::ZERO;
                    let mut op_scalar = 0u32;
                    let mut op_const = 0u64;

                    if let Some(l1_tx) = txs_in_block.first() {
                        if l1_tx.ty() as u8 == 126u8 {
                            is_fjord = true;
                            let data = l1_tx.input().as_ref();
                            if let Some(s) = data.get(4..8) {
                                base_fee_scalar = u32::from_be_bytes(<[u8; 4]>::try_from(s).unwrap_or_default());
                            }
                            if let Some(s) = data.get(8..12) {
                                blob_fee_scalar = u32::from_be_bytes(<[u8; 4]>::try_from(s).unwrap_or_default());
                            }
                            if let Some(s) = data.get(36..68) {
                                l1_base_fee = U256::from_be_slice(s);
                            }
                            if let Some(s) = data.get(68..100) {
                                l1_blob_fee = U256::from_be_slice(s);
                            }
                            if let Some(s) = data.get(164..168) {
                                op_scalar = u32::from_be_bytes(<[u8; 4]>::try_from(s).unwrap_or_default());
                            }
                            if let Some(s) = data.get(168..176) {
                                op_const = u64::from_be_bytes(<[u8; 8]>::try_from(s).unwrap_or_default());
                            }
                        }
                    }

                    let mut out_txs = Vec::with_capacity(expected);
                    let mut out_logs: Vec<Log> = Vec::new();
                    let mut prev_cumgas = 0u64;
                    let mut block_log_index = 0u32;

                    for (tx_idx, (tx, sender)) in txs_in_block
                        .iter()
                        .zip(senders_in_block.iter())
                        .enumerate()
                    {
                        let tx_hash_str = format!("0x{:x}", tx.tx_hash());
                        let is_system_tx = tx.ty() as u8 == 126u8;

                        let receipt: &OpReceipt = local_receipts.get(tx_idx)
                            .expect("receipt count pre-validated");
                        let status = if receipt.status() { 1u32 } else { 0u32 };
                        let cur_cumgas = receipt.cumulative_gas_used();
                        let gas_used = cur_cumgas.saturating_sub(prev_cumgas);
                        prev_cumgas = cur_cumgas;

                        for log in receipt.logs().iter() {
                            out_logs.push(Log {
                                tx_hash: tx_hash_str.clone(),
                                tx_index: tx_idx as u32,
                                log_index: block_log_index,
                                address: format!("0x{:x}", log.address),
                                topics: log.data.topics().iter()
                                    .map(|t| format!("0x{:x}", t))
                                    .collect(),
                                data: format!("0x{}", hex::encode(&log.data.data)),
                            });
                            block_log_index += 1;
                        }

                        let l1_fee = if is_fjord && !is_system_tx {
                            let mut enc = Vec::new();
                            tx.encode_2718(&mut enc);
                            fjord_fee_from_encoded(
                                &enc, l1_base_fee, l1_blob_fee,
                                base_fee_scalar, blob_fee_scalar,
                            )
                        } else if !is_fjord && !is_system_tx {
                            U256::from(op_const)
                                + U256::from(op_scalar) * U256::from(gas_used)
                                    / U256::from(1_000_000u64)
                        } else {
                            U256::ZERO
                        };

                        out_txs.push(GrpcTransaction {
                            tx_index: tx_idx as u32,
                            hash: tx_hash_str,
                            from: format!("0x{:x}", sender),
                            to: tx.to().map(|a| format!("0x{:x}", a)).unwrap_or_default(),
                            value: tx.value().to_string(),
                            input: format!("0x{}", hex::encode(tx.input().as_ref())),
                            gas_used,
                            gas_price: tx.effective_gas_price(hdr.base_fee_per_gas()) as u64,
                            tx_type: tx.ty() as u32,
                            status,
                            l1_fee: l1_fee.to_string(),
                            nonce: tx.nonce(),
                            gas_limit: tx.gas_limit(),
                            max_fee_per_gas: tx.max_fee_per_gas() as u64,
                            max_priority_fee: tx.max_priority_fee_per_gas().unwrap_or(0) as u64,
                            mint_wei: if is_system_tx {
                                tx.value().to_string()
                            } else {
                                "0".to_string()
                            },
                            is_system_tx,
                            ..Default::default()
                        });
                    }

                    // FINAL PLAN A: unstick Persistence before full traces.
                    let hdr_number = hdr.number();
                    let hdr_hash = block.hash();
                    ctx.events
                        .send(ExExEvent::FinishedHeight(alloy_eips::BlockNumHash {
                            number: hdr_number,
                            hash: hdr_hash,
                        }))
                        .ok();

                    let permit = match Arc::clone(&trace_slots).try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => {
                            eprintln!(
                                "[worldchain-exex] trace_queue_full block={} — skip traces this block (tank F5 gap/heal); FinishedHeight already emitted",
                                hdr_number
                            );
                            continue;
                        }
                    };

                    let provider = ctx.provider().clone();
                    let evm_config = ctx.evm_config().clone();
                    let block_owned = block.clone();
                    let txs_owned = txs_in_block.clone();
                    let senders_owned = senders_in_block.clone();
                    let tx_ch2 = tx_ch.clone();
                    let mut notif = BlockNotification {
                        number: hdr_number,
                        hash: format!("0x{:x}", hdr_hash),
                        parent_hash: format!("0x{:x}", hdr.parent_hash()),
                        timestamp: hdr.timestamp(),
                        miner: format!("0x{:x}", hdr.beneficiary()),
                        base_fee: hdr.base_fee_per_gas().unwrap_or(0),
                        gas_limit: hdr.gas_limit(),
                        gas_used: hdr.gas_used(),
                        tx_count: out_txs.len() as u32,
                        size_bytes: block.size() as u32,
                        txs: out_txs,
                        logs: out_logs,
                        traces: Vec::new(),
                        ..Default::default()
                    };
                    tokio::spawn(async move {
                        let _permit = permit;
                        let traces = tokio::task::spawn_blocking(move || {
                            collect_block_traces::<Node>(
                                &provider,
                                &evm_config,
                                &block_owned,
                                &txs_owned,
                                &senders_owned,
                            )
                        })
                        .await
                        .unwrap_or_else(|e| {
                            eprintln!("[worldchain-exex] spawn_blocking join error: {:?}", e);
                            vec![]
                        });
                        notif.traces = traces;
                        if tx_ch2.send(notif).is_err() {
                            eprintln!(
                                "[worldchain-exex] broadcast_send_fail block={} — tank F5 gap/heal",
                                hdr_number
                            );
                        }
                    });
                }
                ctx.events
                    .send(ExExEvent::FinishedHeight(committed_chain.tip().num_hash()))
                    .ok();
            }
        } // while notifications
        eprintln!(
            "[worldchain-exex] notifications stream ended — parking (F2 never-finish); will not return Ok(())"
        );
        // Park forever so ExEx task is not reported "finished" to consensus.
        std::future::pending::<()>().await;
        } // loop
        #[allow(unreachable_code)]
        Ok(())
    })
}

fn collect_block_traces<Node>(
    provider: &Node::Provider,
    evm_config: &Node::Evm,
    block: &reth_primitives::RecoveredBlock<
        <<Node::Types as reth::api::NodeTypes>::Primitives as reth_primitives::NodePrimitives>::Block
    >,
    txs: &[<<Node::Types as reth::api::NodeTypes>::Primitives as reth_primitives::NodePrimitives>::SignedTx],
    senders: &[alloy_primitives::Address],
) -> Vec<nodestream::Trace>
where
    Node: reth::api::FullNodeComponents,
    Node::Types: reth::api::NodeTypes,
    <<Node::Types as reth::api::NodeTypes>::Primitives as reth_primitives::NodePrimitives>::SignedTx:
        reth_primitives_traits::SignedTransaction
        + alloy_consensus::Transaction
        + alloy_eips::eip2718::Typed2718
        + alloy_eips::eip2718::Encodable2718
        + reth_primitives_traits::transaction::TxHashRef
        + Clone,
    Node::Provider: reth_provider::StateProviderFactory + Clone,
    Node::Evm: ConfigureEvm<
        Primitives = <Node::Types as reth::api::NodeTypes>::Primitives,
    > + Clone,
{
    use reth_evm::Evm as _;
    use reth_revm::DatabaseCommit;
    use reth_primitives_traits::BlockHeader;

    let hdr = block.header();
    let n = txs.len();
    if n == 0 {
        return vec![];
    }
    let mut out: Vec<nodestream::Trace> = Vec::new();
    let mut counter = 0u32;
    let mut chunk_start = 0usize;
    while chunk_start < n {
        let chunk_end = (chunk_start + TRACE_RO_CHUNK).min(n);
        if !append_traces_chunk::<Node>(
            provider,
            evm_config,
            block,
            txs,
            senders,
            chunk_start,
            chunk_end,
            &mut counter,
            &mut out,
        ) {
            break;
        }
        chunk_start = chunk_end;
    }
    out
}

/// Reopen MDBX RO from parent, replay txs[0..chunk_end), emit traces for [chunk_start, chunk_end).
fn append_traces_chunk<Node>(
    provider: &Node::Provider,
    evm_config: &Node::Evm,
    block: &reth_primitives::RecoveredBlock<
        <<Node::Types as reth::api::NodeTypes>::Primitives as reth_primitives::NodePrimitives>::Block
    >,
    txs: &[<<Node::Types as reth::api::NodeTypes>::Primitives as reth_primitives::NodePrimitives>::SignedTx],
    senders: &[alloy_primitives::Address],
    chunk_start: usize,
    chunk_end: usize,
    counter: &mut u32,
    out: &mut Vec<nodestream::Trace>,
) -> bool
where
    Node: reth::api::FullNodeComponents,
    Node::Types: reth::api::NodeTypes,
    <<Node::Types as reth::api::NodeTypes>::Primitives as reth_primitives::NodePrimitives>::SignedTx:
        reth_primitives_traits::SignedTransaction
        + alloy_consensus::Transaction
        + alloy_eips::eip2718::Typed2718
        + alloy_eips::eip2718::Encodable2718
        + reth_primitives_traits::transaction::TxHashRef
        + Clone,
    Node::Provider: reth_provider::StateProviderFactory + Clone,
    Node::Evm: ConfigureEvm<
        Primitives = <Node::Types as reth::api::NodeTypes>::Primitives,
    > + Clone,
{
    use reth_evm::Evm as _;
    use reth_revm::DatabaseCommit;
    use reth_primitives_traits::BlockHeader;

    let hdr = block.header();
    let parent_hash = alloy_primitives::B256::from(hdr.parent_hash().0);

    let state_provider = match provider.history_by_block_hash(parent_hash) {
        Ok(sp) => sp,
        Err(e) => {
            eprintln!(
                "[worldchain-exex] trace: state unavailable block={} chunk={}..{} err={}",
                hdr.number(),
                chunk_start,
                chunk_end,
                e
            );
            return false;
        }
    };

    let db = StateProviderDatabase::new(state_provider);
    let mut revm_state = State::builder().with_database(db).build();

    let evm_env: EvmEnvFor<Node::Evm> = match evm_config.evm_env(hdr) {
        Ok(env) => env,
        Err(e) => {
            eprintln!(
                "[worldchain-exex] trace: evm_env failed block={} err={:?}",
                hdr.number(),
                e
            );
            return false;
        }
    };

    for (tx_idx, (tx, sender)) in txs.iter().zip(senders.iter()).enumerate() {
        if tx_idx >= chunk_end {
            break;
        }
        let tx_hash_str = format!("0x{:x}", tx.tx_hash());
        let tx_env: TxEnvFor<Node::Evm> =
            evm_config.tx_env(Recovered::new_unchecked(tx.clone(), *sender));

        let mut inspector = TracingInspector::new(TracingInspectorConfig::default_geth());

        let mut evm = evm_config.evm_with_env_and_inspector(
            &mut revm_state,
            evm_env.clone(),
            &mut inspector,
        );

        let result = match evm.transact(tx_env) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "[worldchain-exex] trace: tx failed block={} tx_idx={} err={:?}",
                    hdr.number(),
                    tx_idx,
                    e
                );
                drop(evm);
                continue;
            }
        };
        drop(evm);
        revm_state.commit(result.state);

        if tx_idx >= chunk_start {
            let arena = inspector.into_traces();
            flatten_arena_nodes(
                &arena,
                0,
                tx_idx as u32,
                &tx_hash_str,
                0,
                counter,
                out,
            );
        }
    }

    true
}


fn flatten_arena_nodes(
    arena: &revm_inspectors::tracing::CallTraceArena,
    node_idx: usize,
    tx_idx: u32,
    tx_hash: &str,
    depth: u32,
    counter: &mut u32,
    out: &mut Vec<nodestream::Trace>,
) {
    let nodes = arena.nodes();
    if node_idx >= nodes.len() { return; }
    let node = &nodes[node_idx];
    let trace = &node.trace;

    let trace_index = *counter;
    *counter += 1;

    let revert_reason = if !trace.success && !trace.output.is_empty() {
        format!("0x{}", hex::encode(&trace.output))
    } else {
        String::new()
    };

    out.push(nodestream::Trace {
        tx_index:      tx_idx,
        tx_hash:       tx_hash.to_string(),
        from_address:  format!("0x{:x}", trace.caller),
        to_address:    format!("0x{:x}", trace.address),
        value_wei:     trace.value.to_string(),
        input_data:    format!("0x{}", hex::encode(&trace.data)),
        output_data:   format!("0x{}", hex::encode(&trace.output)),
        gas:           trace.gas_limit,
        gas_used:      trace.gas_used,
        trace_type:    format!("{:?}", trace.kind).to_lowercase(),
        call_type:     format!("{:?}", trace.kind).to_lowercase(),
        revert_reason,
        trace_index,
    });

    for &child_idx in &node.children {
        flatten_arena_nodes(arena, child_idx, tx_idx, tx_hash, depth + 1, counter, out);
    }
}


fn main() -> Result<()> {
    Cli::parse_args().run(|builder, _| async move {
        builder
            .node(OpNode::default())
            .install_exex("WorldchainStreamer", init_nodestream_exex)
            .launch()
            .await?
            .wait_for_node_exit()
            .await
    })
}
