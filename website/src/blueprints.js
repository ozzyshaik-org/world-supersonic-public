const lineageNodes = {
  blocks: {
    name: 'world_blocks', type: 'RAW EXECUTION', title: 'Committed block context',
    copy: 'The block anchor for the indexed execution record. It keeps an investigation attached to chain context rather than leaving it as an isolated API row.',
    items: ['Block header', 'Commit context', 'World Chain execution'], links: ['transactions', 'traces', 'logs'],
    rule: 'Raw records remain in operator-controlled ClickHouse storage.'
  },
  transactions: {
    name: 'world_transactions', type: 'RAW EXECUTION', title: 'Transaction identity and position',
    copy: 'The transaction record supplies the identity that lets a derived lead and an investigation graph refer to the same onchain action.',
    items: ['Transaction hash', 'Block linkage', 'Transaction position'], links: ['blocks', 'traces', 'logs', 'mev', 'sgip'],
    rule: 'The cross-link invariant is the exact transaction hash.'
  },
  traces: {
    name: 'world_traces', type: 'CANONICAL TRACE DATA', title: 'Execution call context',
    copy: 'Canonical call-trace records power transaction-level investigation. SGIP turns this context into graph elements and transaction metadata.',
    items: ['Call relationships', 'Execution context', 'Transaction linkage'], links: ['transactions', 'sgip'],
    rule: 'A derived classification never replaces the underlying trace evidence.'
  },
  logs: {
    name: 'world_logs', type: 'RAW EXECUTION', title: 'Event context for derivation',
    copy: 'Logs complement transactions and call traces. The ingestion and matching layers use execution records to build useful derived views.',
    items: ['Event records', 'Contract context', 'Transaction linkage'], links: ['transactions', 'mev'],
    rule: 'Derived views should remain inspectable against their raw inputs.'
  },
  mev: {
    name: 'world_mev_activity', type: 'DERIVED FINDING', title: 'A candidate—not a conclusion',
    copy: 'The MEV detector writes derived activity rows after blocks are fully ingested. Those rows can expose a transaction hash that opens the investigation path.',
    items: ['block_number', 'tx_hash', 'mev_type', 'Candidate metadata'], links: ['transactions', 'logs', 'rest', 'sgip'],
    rule: 'Candidates do not prove intent, guaranteed classification or realized profit.'
  },
  rest: {
    name: '/v1/mev/analytics', type: 'REST ACCESS', title: 'Programmatic execution-level lead',
    copy: 'The gateway returns derived MEV findings with block and transaction context, optionally scoped by block or type.',
    items: ['block_number', 'tx_hash', 'mev_type', 'Ingestion context'], links: ['mev', 'sgip'],
    rule: 'The paid event extension plans to charge for this existing route—not invent a new one.'
  },
  sgip: {
    name: '/api/tx/{tx_hash}', type: 'INVESTIGATION GRAPH', title: 'Same transaction, full execution context',
    copy: 'SGIP queries canonical traces and returns Cytoscape graph elements plus transaction metadata for the selected hash.',
    items: ['Call graph', 'Transaction metadata', 'Investigation UI'], links: ['transactions', 'traces', 'mev', 'rest'],
    rule: 'The demo must open the same hash returned by the paid finding.'
  }
};

const sequenceSteps = [
  {
    from: 'Agent', to: 'Supersonic', message: 'GET /v1/mev/analytics', title: 'The seller issues a complete payment challenge.',
    detail: 'The committed design returns HTTP 402 with the complete x402 requirements object. No upstream data is fulfilled before payment acceptance.',
    rule: 'GET-only · empty request body · deterministic request fingerprint'
  },
  {
    from: 'Agent', to: 'Human', message: 'Request fresh operation approval', title: 'Agent Pass checkpoints the exact operation—when required.',
    detail: 'The human freshly authenticates and separately approves or denies the displayed business action. Direct payment without a valid operation or reservation is rejected before settle().',
    rule: 'CONDITIONAL PATH · APPROVAL NEVER REPLACES PAYMENT'
  },
  {
    from: 'Agent', to: 'Supersonic', message: 'Retry with signed authorization', title: 'The request carries payment proof and the operation binding.',
    detail: 'The operation identifier travels in a request header, not in the request fingerprint. The approved payment snapshot remains authoritative.',
    rule: 'auth_key = network | payer | nonce · nonce reuse is request-bound'
  },
  {
    from: 'Supersonic', to: 'Gateway', message: 'seller settle()', title: 'Reservation commits before the seller submits payment.',
    detail: 'The reservation consumes approval, creates the one-to-one payment binding and ledger row, then commits. Only after that commit does the seller call Circle Gateway settle().',
    rule: 'SETTLE BEFORE SERVE · NO CUSTOM SELLER BATCH WORKER'
  },
  {
    from: 'Gateway', to: 'Supersonic', message: 'Acceptance for fulfillment', title: 'Payment acceptance opens the fulfillment boundary.',
    detail: 'Circle Gateway verifies, locks and queues the transfer. received alone is not direct batching evidence; batched or confirmed must be observed for that claim.',
    rule: 'FULFILL AFTER ACCEPTANCE · OBSERVE STATUS HONESTLY'
  },
  {
    from: 'Supersonic', to: 'Agent', message: 'Finding + transaction hash', title: 'The paid response becomes an investigation lead.',
    detail: 'The response is fulfilled once and stored for an identical retry. A permanent post-acceptance upstream failure is paid_unfulfilled rather than silently retried as if unpaid.',
    rule: 'ONE LOGICAL FULFILLED RESPONSE · AT MOST ONE SETTLEMENT'
  },
  {
    from: 'Agent', to: 'SGIP', message: 'GET /api/tx/{tx_hash}', title: 'The exact hash opens canonical execution evidence.',
    detail: 'The investigator sees the paid finding, block context, receipt and SGIP call graph together. The graph is not re-scoped to a different transaction.',
    rule: 'PAID LEAD → SAME HASH → EXECUTION GRAPH'
  },
  {
    from: 'Gateway', to: 'Arc', message: 'Later batch + onchain settlement', title: 'Circle Gateway settles later; the seller only observes.',
    detail: 'Batching and onchain settlement are Gateway responsibilities. Only completed counts as locally settled. A late matching transfer can make an uncertain submission accepted.',
    rule: 'received ≠ batching evidence · completed = locally settled'
  }
];

const stateNodes = {
  submission_pending: { lane: 'PAYMENT STATE', title: 'submission_pending', copy: 'The ledger row exists, but no settlement attempt has been recorded.', rule: 'Automatic void is possible only with attempt counter zero after validity closes.' },
  submission_uncertain: { lane: 'PAYMENT STATE', title: 'submission_uncertain', copy: 'An attempt was made, but its outcome cannot be proven from the available response or lookup.', rule: 'Never auto-void. Empty transfer lookups are not proof of non-acceptance.' },
  accepted: { lane: 'PAYMENT STATE', title: 'accepted', copy: 'A matching accepted payment allows the seller to fulfill the stored request.', rule: 'A late matching transfer becomes accepted even after validity closes.' },
  rejected: { lane: 'PAYMENT STATE', title: 'rejected', copy: 'A definite payment rejection closes this payment attempt.', rule: 'Agent Pass approval is consumed; a new operation requires proposal, approval and authorization.' },
  void: { lane: 'PAYMENT STATE', title: 'void', copy: 'A narrow terminal state for an unsubmitted authorization whose validity has closed.', rule: 'submission_uncertain → void is prohibited.' },
  none: { lane: 'GATEWAY STATUS', title: 'none', copy: 'No Gateway processing status has been observed for this transfer.', rule: 'An absent status proves neither acceptance nor non-acceptance.' },
  received: { lane: 'GATEWAY STATUS', title: 'received', copy: 'Circle Gateway has received the transfer for processing.', rule: 'Not direct evidence of batching and not locally settled.' },
  batched: { lane: 'GATEWAY STATUS', title: 'batched', copy: 'The transfer has entered Gateway batching.', rule: 'Direct batching evidence when actually observed.' },
  confirmed: { lane: 'GATEWAY STATUS', title: 'confirmed', copy: 'Gateway reports confirmation in its settlement workflow.', rule: 'Direct batching evidence, but only completed counts as locally settled.' },
  completed: { lane: 'GATEWAY STATUS', title: 'completed', copy: 'Gateway reports the settlement workflow complete.', rule: 'The only Gateway status counted as locally settled.' },
  failed: { lane: 'GATEWAY STATUS', title: 'failed', copy: 'Gateway failure has different consequences before and after fulfillment.', rule: 'Before fulfillment: payment_failed. After fulfillment: fulfilled_but_unpaid with response preserved.' },
  not_started: { lane: 'FULFILLMENT', title: 'not_started', copy: 'No upstream fulfillment has begun.', rule: 'Gateway failed here prevents fulfillment.' },
  in_progress: { lane: 'FULFILLMENT', title: 'in_progress', copy: 'The upstream read is executing under the fulfillment lifecycle.', rule: 'Lease fencing prevents competing workers from producing divergent responses.' },
  fulfilled: { lane: 'FULFILLMENT', title: 'fulfilled', copy: 'The logical response is stored and available for an identical retry.', rule: 'Retry returns the stored response without another charge.' },
  retryable_failure: { lane: 'FULFILLMENT', title: 'retryable_failure', copy: 'The upstream failure is safe to retry under the existing fulfilled-response lifecycle.', rule: 'A retry must not create another settlement or re-resolve an ENS name.' },
  failed_permanent: { lane: 'FULFILLMENT', title: 'failed_permanent', copy: 'The upstream request cannot produce the paid response permanently.', rule: 'Post-acceptance permanent failure produces paid_unfulfilled.' }
};

export function initBlueprints() {
  const content = document.getElementById('blueprint-content');
  const tabs = [...document.querySelectorAll('[data-blueprint]')];
  let active = 'lineage';
  let selectedLineage = 'mev';
  let selectedSequence = 0;
  let selectedState = 'submission_uncertain';
  let sequenceTimer = null;

  function inspector({ kicker, title, copy, rule, items = [] }) {
    return `<aside class="diagram-inspector"><span class="mono-label">${kicker}</span><h3>${title}</h3><p>${copy}</p>${items.length ? `<div class="diagram-tags">${items.map((item) => `<code>${item}</code>`).join('')}</div>` : ''}<div class="diagram-rule"><span>BOUNDARY</span><strong>${rule}</strong></div></aside>`;
  }

  function renderLineage() {
    const column = (title, keys) => `<div class="lineage-column"><span class="lineage-column-title">${title}</span>${keys.map((key) => {
      const node = lineageNodes[key];
      return `<button class="lineage-node" type="button" data-lineage="${key}" aria-pressed="false"><span>${node.type}</span><strong>${node.name}</strong><small>${node.title}</small></button>`;
    }).join('')}</div>`;
    content.innerHTML = `<div class="lineage-layout"><div class="lineage-map">${column('RAW EXECUTION', ['blocks', 'transactions', 'traces', 'logs'])}<span class="lineage-arrow" aria-hidden="true">→</span>${column('DERIVED', ['mev'])}<span class="lineage-arrow" aria-hidden="true">→</span>${column('ACCESS', ['rest'])}<span class="lineage-arrow" aria-hidden="true">→</span>${column('INVESTIGATE', ['sgip'])}</div><div id="diagram-inspector-mount"></div></div>`;
    updateLineage();
  }

  function updateLineage() {
    const node = lineageNodes[selectedLineage];
    content.querySelectorAll('[data-lineage]').forEach((button) => {
      const key = button.dataset.lineage;
      const selected = key === selectedLineage;
      button.classList.toggle('active', selected);
      button.classList.toggle('related', !selected && node.links.includes(key));
      button.setAttribute('aria-pressed', String(selected));
    });
    content.querySelector('#diagram-inspector-mount').innerHTML = inspector({ kicker: node.type, title: node.title, copy: node.copy, rule: node.rule, items: node.items });
  }

  function renderSequence() {
    content.innerHTML = `<div class="sequence-layout"><div class="sequence-diagram"><div class="sequence-diagram-head"><span>ACTOR → ACTOR / PLANNED REQUEST</span><button class="diagram-play" type="button" data-sequence-play aria-pressed="false">Play diagram</button></div><div class="sequence-steps">${sequenceSteps.map((step, index) => `<button class="sequence-step" type="button" data-sequence="${index}" aria-pressed="false"><span class="sequence-number">${String(index + 1).padStart(2, '0')}</span><span class="sequence-route"><b>${step.from}</b><i>→</i><b>${step.to}</b></span><span class="sequence-message">${step.message}</span></button>`).join('')}</div></div><div id="diagram-inspector-mount"></div></div>`;
    updateSequence();
  }

  function updateSequence() {
    const step = sequenceSteps[selectedSequence];
    content.querySelectorAll('[data-sequence]').forEach((button) => {
      const index = Number(button.dataset.sequence);
      button.classList.toggle('active', index === selectedSequence);
      button.classList.toggle('complete', index < selectedSequence);
      button.setAttribute('aria-pressed', String(index === selectedSequence));
    });
    content.querySelector('#diagram-inspector-mount').innerHTML = inspector({ kicker: `STEP ${String(selectedSequence + 1).padStart(2, '0')} / PLANNED PRODUCT FLOW`, title: step.title, copy: step.detail, rule: step.rule, items: [`${step.from} → ${step.to}`, step.message] });
  }

  function stopSequence() {
    if (sequenceTimer) window.clearInterval(sequenceTimer);
    sequenceTimer = null;
    const button = content.querySelector('[data-sequence-play]');
    if (button) {
      button.textContent = 'Play diagram';
      button.setAttribute('aria-pressed', 'false');
    }
  }

  function renderStates() {
    const lane = (title, keys, note, relations) => `<section class="state-lane"><div class="state-lane-head"><span>${title}</span><small>${note}</small></div><div class="state-lane-nodes">${keys.map((key) => `<button class="state-node" type="button" data-state="${key}" aria-pressed="false">${key}</button>`).join('')}</div><div class="state-relations" aria-label="${title.toLowerCase()} transition rules">${relations.map((relation) => `<code>${relation}</code>`).join('')}</div></section>`;
    content.innerHTML = `<div class="states-layout"><div class="state-map">${lane('PAYMENT STATE', ['submission_pending', 'submission_uncertain', 'accepted', 'rejected', 'void'], 'Key rules · not a linear chain', [
      'submission_pending → accepted / rejected',
      'submission_pending → submission_uncertain → accepted',
      'submission_pending → void · zero attempts only',
      'Unresolved attempts remain submission_uncertain'
    ])}${lane('GATEWAY STATUS', ['none', 'received', 'batched', 'confirmed', 'completed', 'failed'], 'Observe; do not invent', [
      'none → received → batched → confirmed → completed',
      'failed before fulfillment → payment_failed',
      'failed after fulfillment → fulfilled_but_unpaid'
    ])}${lane('FULFILLMENT STATE', ['not_started', 'in_progress', 'fulfilled', 'retryable_failure', 'failed_permanent'], 'One stored logical response', [
      'not_started → in_progress → fulfilled',
      'in_progress → retryable_failure → in_progress',
      'in_progress → failed_permanent → paid_unfulfilled'
    ])}<div class="state-prohibition"><span>PROHIBITED SHORTCUT</span><strong>submission_uncertain <i>↛</i> void</strong><small>An attempted uncertain payment requires resolution or operator escalation—not convenient disappearance.</small></div></div><div id="diagram-inspector-mount"></div></div>`;
    updateState();
  }

  function updateState() {
    const node = stateNodes[selectedState];
    content.querySelectorAll('[data-state]').forEach((button) => {
      const selected = button.dataset.state === selectedState;
      button.classList.toggle('active', selected);
      button.setAttribute('aria-pressed', String(selected));
    });
    content.querySelector('#diagram-inspector-mount').innerHTML = inspector({ kicker: node.lane, title: node.title, copy: node.copy, rule: node.rule });
  }

  function render() {
    stopSequence();
    tabs.forEach((tab) => {
      const selected = tab.dataset.blueprint === active;
      tab.classList.toggle('active', selected);
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
    });
    content.setAttribute('aria-labelledby', `blueprint-tab-${active}`);
    if (active === 'lineage') renderLineage();
    else if (active === 'sequence') renderSequence();
    else renderStates();
    const status = active === 'lineage'
      ? 'BASELINE DATA MODEL · CONCEPTUAL MAP'
      : active === 'sequence' ? 'PLANNED PRODUCT FLOW · NOT A LIVE RUN' : 'PLANNED PAYMENT RULES · SPECIFICATION';
    document.getElementById('blueprint-status').innerHTML = `<span class="status-dot"></span>${status}`;
  }

  tabs.forEach((tab) => tab.addEventListener('click', () => { active = tab.dataset.blueprint; render(); }));
  document.querySelector('.blueprint-tabs').addEventListener('keydown', (event) => {
    if (!['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const current = tabs.findIndex((tab) => tab.dataset.blueprint === active);
    let next = current;
    if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabs.length - 1;
    else if (event.key === 'ArrowRight') next = (current + 1) % tabs.length;
    else next = (current - 1 + tabs.length) % tabs.length;
    active = tabs[next].dataset.blueprint;
    render();
    tabs[next].focus();
  });

  content.addEventListener('click', (event) => {
    const lineage = event.target.closest('[data-lineage]');
    if (lineage) { selectedLineage = lineage.dataset.lineage; updateLineage(); return; }
    const sequence = event.target.closest('[data-sequence]');
    if (sequence) { stopSequence(); selectedSequence = Number(sequence.dataset.sequence); updateSequence(); return; }
    const play = event.target.closest('[data-sequence-play]');
    if (play) {
      if (sequenceTimer) { stopSequence(); return; }
      play.textContent = 'Pause diagram';
      play.setAttribute('aria-pressed', 'true');
      sequenceTimer = window.setInterval(() => {
        selectedSequence = (selectedSequence + 1) % sequenceSteps.length;
        updateSequence();
      }, 1050);
      return;
    }
    const state = event.target.closest('[data-state]');
    if (state) { selectedState = state.dataset.state; updateState(); }
  });

  render();
  return { stopSequence };
}
