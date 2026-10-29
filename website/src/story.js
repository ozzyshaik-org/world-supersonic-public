import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';

gsap.registerPlugin(ScrollTrigger);

const chapters = [
  { label: '01 / DERIVED LEAD', aria: 'Synthetic derived MEV candidate for transaction A' },
  { label: '02 / EXECUTION GRAPH', aria: 'Synthetic call graph for the same transaction A' },
  { label: '03 / EVIDENCE LINEAGE', aria: 'Raw records and derived evidence linked to transaction A' },
  { label: '04 / INSPECTABLE RESPONSE', aria: 'Synthetic response preserving transaction context and evidence boundaries' }
];

function leadScene() {
  return `<div class="story-scene story-lead-scene">
    <div class="story-scene-query"><span>GET</span><code>/v1/mev/analytics</code><small>DERIVED DATASET</small></div>
    <div class="story-lead-head"><span>TRANSACTION</span><span>PATTERN</span><span>STATUS</span></div>
    <div class="story-lead-row"><code>SYNTHETIC TX C</code><span>ATOMIC ROUTE</span><small>CANDIDATE</small></div>
    <div class="story-lead-row is-selected"><code>SYNTHETIC TX A</code><span>TWO VENUE ROUTE</span><small>SELECTED LEAD</small></div>
    <div class="story-lead-row"><code>SYNTHETIC TX B</code><span>ORDERING CONTEXT</span><small>CANDIDATE</small></div>
    <div class="story-scene-note"><strong>The detector opens the question.</strong><span>It does not prove intent or realized profit.</span></div>
  </div>`;
}

function traceScene() {
  const nodes = [
    { x: 70, y: 180, label: 'ENTRY' },
    { x: 215, y: 82, label: 'ROUTER A' },
    { x: 215, y: 278, label: 'ROUTER B' },
    { x: 380, y: 82, label: 'PAIR A' },
    { x: 380, y: 278, label: 'PAIR B' },
    { x: 535, y: 180, label: 'RETURN' }
  ];
  const edges = [[0, 1], [0, 2], [1, 3], [2, 4], [3, 5], [4, 5]];
  const edgeMarkup = edges.map(([from, to]) => {
    const a = nodes[from];
    const b = nodes[to];
    const middle = (a.x + b.x) / 2;
    return `<path d="M ${a.x} ${a.y} C ${middle} ${a.y}, ${middle} ${b.y}, ${b.x} ${b.y}" />`;
  }).join('');
  const nodeMarkup = nodes.map((node, index) => `<g class="story-graph-node" transform="translate(${node.x} ${node.y})"><circle r="24"></circle><text class="story-node-number" y="4">${index + 1}</text><text class="story-node-label" y="47">${node.label}</text></g>`).join('');
  return `<div class="story-scene story-trace-scene">
    <div class="story-same-hash"><span>SELECTED LEAD</span><code>SYNTHETIC TX A</code><small>SAME TRANSACTION</small></div>
    <svg viewBox="0 0 610 370" aria-hidden="true"><g class="story-graph-edges">${edgeMarkup}</g>${nodeMarkup}</svg>
    <div class="story-scene-note"><strong>/api/tx/{tx_hash}</strong><span>Entry, routers, venues, and return stay in one execution context.</span></div>
  </div>`;
}

function verifyScene() {
  return `<div class="story-scene story-verify-scene">
    <div class="story-same-hash"><span>EVIDENCE LINEAGE</span><code>SYNTHETIC TX A</code><small>OPERATOR-CONTROLLED CLICKHOUSE</small></div>
    <div class="story-lineage">
      <div><span>RAW EXECUTION</span><code>world_traces</code><small>Call path and execution context</small></div>
      <div><span>RAW EXECUTION</span><code>world_logs</code><small>Events used by derived matchers</small></div>
      <div class="is-derived"><span>DERIVED FINDING</span><code>world_mev_activity</code><small>A candidate linked back to the same hash</small></div>
    </div>
    <div class="story-boundary"><span>CLASSIFICATION BOUNDARY</span><strong>Candidate ≠ intent ≠ realized profit</strong><p>The raw evidence remains queryable so the derived row can be challenged.</p></div>
  </div>`;
}

function respondScene() {
  return `<div class="story-scene story-respond-scene">
    <div class="story-same-hash"><span>STRUCTURED RESULT</span><code>SYNTHETIC RESPONSE</code><small>NO LIVE REQUEST</small></div>
    <pre><code>{
  "tx_hash": "SYNTHETIC TX A",
  "block": "SAMPLE BLOCK 001",
  "finding": "ARBITRAGE CANDIDATE",
  "graph": "/api/tx/{synthetic_tx_a}",
  "evidence": ["calls", "logs", "block context"],
  "payment_receipt": "not issued in preview"
}</code></pre>
    <div class="story-response-boundary"><span><strong>Existing core</strong>Finding + graph + evidence context</span><span><strong>Product direction</strong>Paid access + stored-response receipt</span></div>
  </div>`;
}

function sceneMarkup(index) {
  return [leadScene, traceScene, verifyScene, respondScene][index]();
}

export function initStory({ reducedMotion }) {
  const root = document.querySelector('[data-story]');
  if (!root) return;
  const visual = document.getElementById('story-visual');
  const stageLabel = document.getElementById('story-stage-label');
  const progressLine = document.querySelector('.story-stage-progress span');
  const articles = [...root.querySelectorAll('[data-story-step]')];
  const buttons = [...root.querySelectorAll('[data-story-go]')];
  let activeStep = -1;
  let scrollProgress = 0;

  document.querySelectorAll('[data-story-mobile]').forEach((container) => {
    container.innerHTML = sceneMarkup(Number(container.dataset.storyMobile));
  });

  function setStep(index, animate = true) {
    if (index === activeStep) return;
    activeStep = index;
    visual.innerHTML = sceneMarkup(index);
    visual.setAttribute('aria-label', chapters[index].aria);
    stageLabel.textContent = chapters[index].label;
    articles.forEach((article, articleIndex) => article.classList.toggle('is-active', articleIndex === index));
    buttons.forEach((button, buttonIndex) => button.setAttribute('aria-pressed', String(buttonIndex === index)));
    if (animate && !reducedMotion && window.innerWidth >= 900) {
      gsap.fromTo(visual.firstElementChild,
        { opacity: 0.25, y: 22, scale: 0.985 },
        { opacity: 1, y: 0, scale: 1, duration: 0.28, ease: 'power2.out', overwrite: true }
      );
    }
  }

  buttons.forEach((button) => {
    button.addEventListener('click', () => {
      const index = Number(button.dataset.storyGo);
      const target = articles[index];
      const top = target.getBoundingClientRect().top + window.scrollY - window.innerHeight * 0.22;
      setStep(index);
      window.scrollTo({ top, behavior: reducedMotion ? 'instant' : 'smooth' });
    });
  });

  setStep(0, false);
  ScrollTrigger.create({
    trigger: root,
    start: 'top 58%',
    end: 'bottom 42%',
    scrub: reducedMotion ? false : 0.35,
    onUpdate(self) {
      scrollProgress = self.progress;
      progressLine.style.transform = `scaleX(${self.progress})`;
    }
  });
  articles.forEach((article, index) => {
    ScrollTrigger.create({
      trigger: article,
      start: 'top 58%',
      end: 'bottom 42%',
      onEnter: () => setStep(index),
      onEnterBack: () => setStep(index)
    });
  });
  document.fonts?.ready.then(() => ScrollTrigger.refresh());

  window.__STORY_STATS__ = () => ({
    step: activeStep,
    progress: scrollProgress,
    chapters: articles.length,
    desktopStageVisible: getComputedStyle(document.querySelector('.story-stage-column')).display !== 'none',
    mobileScenes: document.querySelectorAll('[data-story-mobile] .story-scene').length
  });
}
