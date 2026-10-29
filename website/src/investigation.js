// Field-shaped, deliberately synthetic records for the product preview.
// These identifiers are not presented as valid World Chain transactions.
export const findings = [
  {
    id: 'A', filter: 'arbitrage', kind: 'ARBITRAGE CANDIDATE', mevType: 'atomic_arbitrage',
    title: 'Synthetic transaction A', tx: 'SYNTHETIC TX A', block: 'SAMPLE BLOCK 001',
    blockNumber: 1, txIndex: 7, listNote: 'TWO VENUE ROUTE', uniqueAddresses: 6,
    nodes: [
      ['ENTRY', 'Entry call opens the synthetic execution path.'],
      ['ROUTER A', 'The first router leg is visible in the call-trace model.'],
      ['PAIR A', 'A synthetic venue interaction provides the first route leg.'],
      ['ROUTER B', 'The second router leg remains linked to the same transaction.'],
      ['PAIR B', 'A second synthetic venue interaction completes the route pattern.'],
      ['RETURN', 'Control returns to the entry context; no profit claim is inferred.']
    ]
  },
  {
    id: 'B', filter: 'sandwich', kind: 'SANDWICH CANDIDATE', mevType: 'sandwich_candidate',
    title: 'Synthetic transaction B', tx: 'SYNTHETIC TX B', block: 'SAMPLE BLOCK 002',
    blockNumber: 2, txIndex: 11, listNote: 'VICTIM CONTEXT', uniqueAddresses: 6,
    nodes: [
      ['ENTRY', 'The synthetic candidate begins with an externally initiated call.'],
      ['TOKEN IN', 'An input-token transfer is shown as illustrative context.'],
      ['ROUTER', 'The router call is the next inspectable execution step.'],
      ['PAIR', 'The synthetic pair interaction carries the candidate pattern.'],
      ['TOKEN OUT', 'An output-token leg completes the illustrated route.'],
      ['VICTIM LINK', 'Optional victim context is shown; intent is not proven.']
    ]
  },
  {
    id: 'C', filter: 'arbitrage', kind: 'ARBITRAGE CANDIDATE', mevType: 'atomic_arbitrage',
    title: 'Synthetic transaction C', tx: 'SYNTHETIC TX C', block: 'SAMPLE BLOCK 003',
    blockNumber: 3, txIndex: 4, listNote: 'ATOMIC ROUTE', uniqueAddresses: 6,
    nodes: [
      ['ENTRY', 'The preview starts at the synthetic transaction entrypoint.'],
      ['FLASH CALL', 'An illustrative flash call introduces temporary execution context.'],
      ['VENUE X', 'The first synthetic venue call is preserved in the trace.'],
      ['VENUE Y', 'The second synthetic venue call follows within the same transaction.'],
      ['REPAY', 'The illustrated repayment step closes the temporary context.'],
      ['RETURN', 'The route closes without asserting a realized value.']
    ]
  },
  {
    id: 'D', filter: 'sandwich', kind: 'SANDWICH CANDIDATE', mevType: 'sandwich_candidate',
    title: 'Synthetic transaction D', tx: 'SYNTHETIC TX D', block: 'SAMPLE BLOCK 004',
    blockNumber: 4, txIndex: 19, listNote: 'ORDERING LEAD', uniqueAddresses: 6,
    nodes: [
      ['ENTRY', 'The synthetic lead begins with an entry call.'],
      ['APPROVE', 'An illustrative token approval appears in the call sequence.'],
      ['ROUTER', 'The router call anchors the candidate to a transaction.'],
      ['POOL', 'A pool interaction supplies the next execution context.'],
      ['TRANSFER', 'The transfer step is inspectable alongside canonical traces.'],
      ['CONTEXT', 'Ordering remains a derived lead, not an intent classification.']
    ]
  }
];

export function derivedResponse(finding) {
  return {
    block_number: finding.blockNumber,
    tx_hash: finding.tx,
    tx_index: finding.txIndex,
    mev_type: finding.mevType,
    extracted_value_wei: 'SYNTHETIC ESTIMATE — NOT REALIZED PROFIT',
    sandwich_victim_tx: finding.filter === 'sandwich' ? 'SYNTHETIC VICTIM CONTEXT' : '',
    ingested_at: 'SYNTHETIC TIMESTAMP'
  };
}

export function derivedResponsePayload(finding) {
  return [derivedResponse(finding)];
}

export function sgipMetadata(finding) {
  return {
    tx_hash: finding.tx,
    total_traces: finding.nodes.length,
    unique_addresses: finding.uniqueAddresses
  };
}
