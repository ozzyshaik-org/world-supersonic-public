const REPO = 'https://github.com/ozzyshaik-org/world-supersonic-public';
const COMMIT = `${REPO}/blob/30b9caacee9916a103401255e844c1e78bf1b157`;

export const slides = [
  {
    short: 'Thesis', duration: 20, status: 'PRODUCT THESIS', kicker: 'SUPERSONIC / WORLD CHAIN 480',
    title: 'See what the block <em>actually</em> did.',
    body: 'Execution-level indexing and investigation infrastructure—from committed World Chain blocks to evidence a person or agent can inspect.',
    meta: 'OPEN-SOURCE CORE · OPERATOR-CONTROLLED DATA · SYNTHETIC PRODUCT PREVIEW',
    visual: `<div class="pitch-thesis-visual"><div class="pitch-signal"><span>COMMITTED EXECUTION</span><strong>One transaction identity</strong><small>Blocks · transactions · logs · call traces</small></div><div class="pitch-thesis-path"><span>RETH EXEX</span><i>→</i><span>CLICKHOUSE</span><i>→</i><span>REST + SGIP</span></div><div class="pitch-answer"><span>INSPECTABLE ANSWER</span><strong>Finding + execution graph + evidence context</strong></div></div>`,
    notes: 'Open on the product, not the hackathon. Supersonic connects execution indexing, derived findings and transaction investigation in one operator-controlled World Chain stack.',
    source: { label: 'Repository overview', url: REPO }
  },
  {
    short: 'Problem', duration: 25, status: 'THE INVESTIGATION GAP', kicker: 'THE PROBLEM / AFTER FINALITY',
    title: 'A hash tells you where to look. <em>Not what happened.</em>',
    body: 'A final transaction still leaves the investigator asking which calls executed, why the activity was flagged, and whether somebody else can inspect the same evidence.',
    visual: `<div class="pitch-fragments"><div><span>01 / ALERT</span><strong>A derived candidate</strong><small>Why was it flagged?</small></div><div><span>02 / EXECUTION</span><strong>Calls and logs</strong><small>Where is the connected context?</small></div><div><span>03 / INSPECTION</span><strong>A transaction graph</strong><small>Can the finding be challenged?</small></div><p>Three questions. Too often, three separate tools.</p></div>`,
    notes: 'The pain is not another missing RPC endpoint. It is the investigative gap between an alert and a defensible explanation of the exact transaction.',
    source: { label: 'SGIP transaction route', url: `${COMMIT}/sgip/main.py#L259-L343` }
  },
  {
    short: 'Investigation', duration: 35, status: 'SYNTHETIC WALKTHROUGH', kicker: 'ONE TRANSACTION / FOUR MOMENTS',
    title: 'Lead → execution → verification → <em>response.</em>',
    body: 'Follow one deliberately synthetic candidate without losing its transaction identity. The detector starts the question; raw execution evidence sets the boundary of the answer.',
    meta: 'SYNTHETIC TX A · SAMPLE BLOCK 001 · NO LIVE CHAIN CALL',
    visual: `<div class="pitch-case"><div class="pitch-case-hash"><span>CASE IDENTITY</span><code>SYNTHETIC TX A</code></div><div class="pitch-case-steps"><div><span>01</span><strong>Detect</strong><small>atomic_arbitrage candidate</small></div><div><span>02</span><strong>Trace</strong><small>6 illustrative call steps</small></div><div><span>03</span><strong>Verify</strong><small>world_traces + world_logs</small></div><div><span>04</span><strong>Respond</strong><small>Finding + graph + context</small></div></div><p><strong>Candidate ≠ intent ≠ realized profit</strong></p></div>`,
    notes: 'This is the product moment. Select a derived lead, open the same transaction in SGIP, inspect canonical trace context, and preserve a structured answer. Everything in this walkthrough is labelled synthetic.',
    source: { label: 'MEV response fields', url: `${COMMIT}/cmd/rpc-gateway/main.go#L82-L90` }
  },
  {
    short: 'Difference', duration: 25, status: 'PRODUCT DIFFERENCE', kicker: 'WHY SUPERSONIC / SPECIALIZED BY DESIGN',
    title: 'Less stitching. <em>More investigating.</em>',
    body: 'Broader platforms can support equivalent workflows. Supersonic’s distinction is an opinionated World Chain path from execution records to a derived lead and its investigation graph.',
    visual: `<div class="pitch-lanes"><div><span>RPC + STREAMS</span><strong>Access primitives</strong><small>Supersonic adds raw, derived and investigation layers.</small></div><div><span>INDEXING + ANALYTICS</span><strong>Programmability and research</strong><small>Supersonic specializes around World Chain execution.</small></div><div><span>SIMULATION + DEBUGGING</span><strong>Transaction operations</strong><small>Supersonic emphasizes derived discovery and inspection.</small></div><p>RETH EXEX → RAW EVIDENCE → DERIVED LEAD → SGIP</p></div>`,
    notes: 'Do not claim that competitors cannot build this workflow. The advantage is integration, World Chain specialization and a shorter path to an inspectable answer.',
    source: { label: 'Architecture and known limits', url: `${COMMIT}/README.md#architecture` }
  },
  {
    short: 'Architecture', duration: 30, status: 'EXISTING PUBLIC CORE', kicker: 'ARCHITECTURE / OPERATOR CONTROL',
    title: 'One operator-controlled <em>execution pipeline.</em>',
    body: 'The public core integrates extraction, ingestion, raw and derived storage, programmatic access and investigation UI.',
    visual: `<div class="pitch-stack"><div><span>01</span><strong>Reth Execution Extension</strong><code>exex/ · gRPC stream</code></div><div><span>02</span><strong>tank-engine + MEV detector</strong><code>Ingest · heal · derive</code></div><div><span>03</span><strong>ClickHouse + Valkey</strong><code>Raw world_* + derived datasets</code></div><div><span>04</span><strong>REST + WebSocket + SGIP</strong><code>Access + investigation</code></div></div>`,
    notes: 'Walk the pipeline from inside op-reth to SGIP. Keep the raw evidence beside the derived datasets so a finding does not become a black box.',
    source: { label: 'Execution Extension source', url: `${COMMIT}/exex/src/main.rs` }
  },
  {
    short: 'Scale', duration: 20, status: 'DATED OPERATOR MEASUREMENT', kicker: 'SCALE / 24 OCTOBER 2026',
    title: 'Built around execution at <em>scale.</em>',
    body: 'Self-reported, read-only production measurements show the scale of the operating dataset. They are dated readings—not live counters or independent benchmarks.',
    visual: `<div class="pitch-metrics"><div><strong>≈17.6<span>B</span></strong><small>CALL TRACES</small></div><div><strong>≈720<span>M</span></strong><small>TRANSACTIONS</small></div><div><strong>≈30<span>B</span></strong><small>ROWS ACROSS ~55 TABLES</small></div><p>Measured read-only against the production deployment on 24 Oct 2026.</p></div>`,
    notes: 'Use the figures with their date and provenance. They demonstrate operating context, not comparative performance or a live website connection.',
    source: { label: 'Dated measurements and limitations', url: `${COMMIT}/README.md#what-is-live-and-what-is-not-known-limits` }
  },
  {
    short: 'Roadmap', duration: 30, status: 'PLANNED · NOT IMPLEMENTED', kicker: 'PRODUCT ROADMAP / THE SAME INVESTIGATION',
    title: 'Make the answer <em>payable and permissioned.</em>',
    body: 'The planned extensions strengthen one workflow: request an investigation, authorize it safely, follow the evidence, and replay the fulfilled response.',
    visual: `<div class="pitch-roadmap"><div><span>ARC / X402</span><strong>402 → settle() → fulfill</strong><small>Circle Gateway batches later.</small></div><div><span>IDKIT</span><strong>Human-gated free-tier key</strong><small>One active self-service key per action-scoped nullifier.</small></div><div><span>AGENT PASS</span><strong>Fresh approval for one action</strong><small>Approval never replaces payment.</small></div><div><span>ENS</span><strong>Resolve once, pin through fulfillment</strong><small>Only two address routes are in scope.</small></div></div>`,
    notes: 'These are specifications, not live capabilities. Each planned integration must improve the same investigation; this product site is not the Arc Studio event frontend or partner evidence.',
    source: { label: 'Planned capability specifications', url: `${COMMIT}/README.md#product-direction` }
  },
  {
    short: 'Proof', duration: 30, status: 'REPRODUCIBILITY + BOUNDARIES', kicker: 'PRODUCT PROOF / INSPECT THE CLAIMS',
    title: 'Trust the workflow because you can <em>inspect it.</em>',
    body: 'Source references connect the story to public code. Dated measurements, synthetic records and planned capabilities remain visibly different states.',
    visual: `<div class="pitch-proof"><a href="${COMMIT}/cmd/rpc-gateway/main.go#L1049-L1127" target="_blank" rel="noreferrer"><span>DERIVED API</span><strong>GetMevAnalyticsHandler</strong><i>↗</i></a><a href="${COMMIT}/sgip/main.py#L259-L343" target="_blank" rel="noreferrer"><span>INVESTIGATION</span><strong>/api/tx/{tx_hash}</strong><i>↗</i></a><a href="${COMMIT}/exex/README.md#L7-L20" target="_blank" rel="noreferrer"><span>RUNTIME BOUNDARY</span><strong>Compile-verified ≠ runtime parity</strong><i>↗</i></a><p>Next proof milestone: a labelled public fixture and one-command REST/SGIP verification. Planned, not yet run.</p></div>`,
    notes: 'A polished preview is not the evidence. Evaluators can inspect the implementation and its limits now; the event milestone is a reproducible fixture with a real verification run.',
    source: { label: 'Evidence register', url: `${COMMIT}/EVIDENCE.md` }
  },
  {
    short: 'Ambition', duration: 25, status: 'THE AMBITION', kicker: 'SUPERSONIC / WORLD CHAIN EXECUTION INTELLIGENCE',
    title: 'Build the default investigation layer for <em>World Chain execution.</em>',
    body: 'Not another isolated endpoint: the shortest path from a chain-level signal to evidence a person or agent can inspect.',
    meta: 'EXISTING CORE · EXPLICIT LIMITS · ONE CONNECTED PRODUCT DIRECTION',
    visual: `<div class="pitch-closing"><p>One memorable investigation is the thesis:<br><strong>derived finding → same-transaction graph → inspectable evidence.</strong></p><button class="button button-primary" type="button" data-pitch-workspace>Open the investigation workspace <i data-lucide="arrow-right" aria-hidden="true"></i></button><a class="text-link" href="${REPO}" target="_blank" rel="noreferrer">Inspect the open-source core <i data-lucide="arrow-up-right" aria-hidden="true"></i></a></div>`,
    notes: 'Close on conviction and invite inspection. The immediate product proof is the connected investigation; the roadmap makes that workflow easier to request, authorize and replay.',
    source: { label: 'Open-source repository', url: REPO }
  }
];

export const PITCH_TOTAL_SECONDS = slides.reduce((total, slide) => total + slide.duration, 0);
