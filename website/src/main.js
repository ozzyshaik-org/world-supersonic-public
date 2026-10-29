import './style.css';
import './showcase.css';
import './story.css';
import './pitch.css';
import {
  createIcons, ArrowUpRight, ArrowDownRight, ArrowRight, ArrowLeft, ArrowDown,
  Play, Pause, Menu, X, ScanEye, Braces, Info, RotateCcw, CalendarClock,
  Radar, CircleDollarSign, UserCheck, Workflow, Plus, Code, Copy,
  GitCompareArrows, ListChecks, Clapperboard, Layers3, Box, UnfoldVertical,
  MousePointer2, DatabaseZap, Waypoints, GitBranch, LayoutGrid, NotebookText,
  Maximize2, Minimize2
} from 'lucide';
import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { createExecutionScene } from './scene.js';
import { initCoreShowcase } from './showcase.js';
import { initBlueprints } from './blueprints.js';
import { initStory } from './story.js';
import { findings, derivedResponsePayload, sgipMetadata } from './investigation.js';
import { initPitch } from './pitch.js';

const iconSet = {
  ArrowUpRight, ArrowDownRight, ArrowRight, ArrowLeft, ArrowDown, Play, Pause,
  Menu, X, ScanEye, Braces, Info, RotateCcw, CalendarClock, Radar,
  CircleDollarSign, UserCheck, Workflow, Plus, Code, Copy, GitCompareArrows,
  ListChecks, Clapperboard, Layers3, Box, UnfoldVertical, MousePointer2,
  DatabaseZap, Waypoints, GitBranch, LayoutGrid, NotebookText, Maximize2, Minimize2
};
const refreshIcons = () => createIcons({ icons: iconSet, attrs: { 'stroke-width': 1.6 } });
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
refreshIcons();

const graphPositions = [
  { x: 62, y: 180 }, { x: 188, y: 78 }, { x: 188, y: 282 },
  { x: 364, y: 78 }, { x: 364, y: 282 }, { x: 548, y: 180 }
];
const compactGraphPositions = [
  { x: 150, y: 70 }, { x: 75, y: 165 }, { x: 225, y: 165 },
  { x: 75, y: 275 }, { x: 225, y: 275 }, { x: 150, y: 370 }
];
const graphEdges = [[0, 1], [0, 2], [1, 3], [2, 4], [3, 5], [4, 5]];
let selectedFinding = findings[0];
let traceStep = 0;
let traceTimer = null;

const findingList = document.getElementById('finding-list');
const traceGraph = document.getElementById('trace-graph');
const traceResponse = document.getElementById('trace-response');
const traceControls = document.getElementById('trace-controls');
const traceScrubber = document.getElementById('trace-scrubber');
const tracePlay = document.getElementById('trace-play');
const traceViewButtons = [...document.querySelectorAll('[data-trace-view]')];

function renderFindingList(filter = 'all') {
  const visible = findings.filter((finding) => filter === 'all' || finding.filter === filter);
  findingList.innerHTML = visible.length ? visible.map((finding) => `
    <button class="finding-item ${finding.id === selectedFinding.id ? 'active' : ''}" type="button" data-finding="${finding.id}" aria-pressed="${finding.id === selectedFinding.id}">
      <span><strong>${finding.tx}</strong><small>${finding.block}</small></span>
      <span>${finding.listNote}</span>
    </button>`).join('') : '<p class="finding-empty">No synthetic records match this filter.</p>';
}

function traceGraphMarkup(finding) {
  const compact = window.innerWidth < 600;
  const positions = compact ? compactGraphPositions : graphPositions;
  const nodes = finding.nodes.map((node, index) => {
    const position = positions[index];
    return `<g class="graph-node" data-node="${index}" transform="translate(${position.x} ${position.y})">
      <circle r="${compact ? 20 : 24}"></circle><text class="node-index" y="4">${index + 1}</text>
      <text y="43">${node[0]}</text>
    </g>`;
  }).join('');
  const edgeMarkup = graphEdges.map(([from, to]) => {
    const a = positions[from];
    const b = positions[to];
    const midX = (a.x + b.x) / 2;
    return `<path class="graph-edge" data-edge-from="${from}" data-edge-to="${to}" d="M ${a.x} ${a.y} C ${midX} ${a.y}, ${midX} ${b.y}, ${b.x} ${b.y}"></path>`;
  }).join('');
  return `<svg viewBox="${compact ? '0 0 300 430' : '0 0 610 370'}" role="img" aria-labelledby="graph-title graph-desc">
    <title id="graph-title">${finding.title} synthetic call graph</title>
    <desc id="graph-desc">An illustrative six-step execution path. This is not chain data.</desc>
    <text class="graph-caption" x="12" y="24">SYNTHETIC CALL GRAPH · NOT CHAIN DATA</text>
    <g>${edgeMarkup}</g><g>${nodes}</g>
  </svg>`;
}

function updateTraceStep() {
  traceGraph.querySelectorAll('[data-node]').forEach((node) => {
    const index = Number(node.dataset.node);
    node.classList.toggle('active', index === traceStep);
    node.classList.toggle('complete', index < traceStep);
  });
  traceGraph.querySelectorAll('[data-edge-to]').forEach((edge) => {
    edge.classList.toggle('active', Number(edge.dataset.edgeTo) <= traceStep && Number(edge.dataset.edgeFrom) < traceStep);
  });
  traceScrubber.value = String(traceStep);
  document.getElementById('trace-output').textContent = `${traceStep + 1} / ${selectedFinding.nodes.length}`;
  document.getElementById('step-explanation').textContent = selectedFinding.nodes[traceStep][1];
}

function stopTrace() {
  if (traceTimer) window.clearInterval(traceTimer);
  traceTimer = null;
  tracePlay.setAttribute('aria-label', 'Play trace');
  tracePlay.innerHTML = '<i data-lucide="play" aria-hidden="true"></i>';
  refreshIcons();
}

function renderResponses() {
  document.getElementById('derived-response-json').textContent = JSON.stringify(derivedResponsePayload(selectedFinding), null, 2);
  document.getElementById('sgip-response-json').textContent = JSON.stringify(sgipMetadata(selectedFinding), null, 2);
}

function selectTraceView(view, focus = false) {
  const showGraph = view === 'graph';
  if (!showGraph) stopTrace();
  traceGraph.hidden = !showGraph;
  traceResponse.hidden = showGraph;
  traceControls.hidden = !showGraph;
  traceViewButtons.forEach((button) => {
    const active = button.dataset.traceView === view;
    button.classList.toggle('active', active);
    button.setAttribute('aria-selected', String(active));
    button.tabIndex = active ? 0 : -1;
    if (active && focus) button.focus();
  });
}

traceViewButtons.forEach((button) => {
  button.addEventListener('click', () => selectTraceView(button.dataset.traceView));
  button.addEventListener('keydown', (event) => {
    if (!['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const current = traceViewButtons.indexOf(button);
    let next = current;
    if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = traceViewButtons.length - 1;
    else if (event.key === 'ArrowRight') next = (current + 1) % traceViewButtons.length;
    else next = (current - 1 + traceViewButtons.length) % traceViewButtons.length;
    selectTraceView(traceViewButtons[next].dataset.traceView, true);
  });
});

document.getElementById('copy-response').addEventListener('click', async (event) => {
  const button = event.currentTarget;
  const label = button.querySelector('span');
  const value = document.getElementById('derived-response-json').textContent;
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
    } else {
      const textarea = document.createElement('textarea');
      textarea.value = value;
      textarea.setAttribute('readonly', '');
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      document.body.append(textarea);
      textarea.select();
      const copied = document.execCommand('copy');
      textarea.remove();
      if (!copied) throw new Error('Copy command was refused');
    }
    label.textContent = 'Copied';
  } catch {
    label.textContent = 'Select JSON to copy';
    document.getElementById('derived-response-json').parentElement.focus();
  }
  window.setTimeout(() => { label.textContent = 'Copy derived JSON'; }, 1800);
});

function selectFinding(id) {
  const next = findings.find((finding) => finding.id === id);
  if (!next) return;
  stopTrace();
  selectedFinding = next;
  traceStep = 0;
  document.getElementById('trace-title').textContent = next.title;
  document.getElementById('trace-kind').textContent = next.kind;
  document.getElementById('evidence-tx').textContent = next.tx;
  document.getElementById('evidence-block').textContent = next.block;
  traceScrubber.max = String(next.nodes.length - 1);
  traceGraph.innerHTML = traceGraphMarkup(next);
  renderResponses();
  const activeFilter = document.querySelector('.filter-button.active')?.dataset.filter || 'all';
  renderFindingList(activeFilter);
  updateTraceStep();
}

findingList.addEventListener('click', (event) => {
  const button = event.target.closest('[data-finding]');
  if (button) selectFinding(button.dataset.finding);
});

document.querySelectorAll('[data-filter]').forEach((button) => {
  button.addEventListener('click', () => {
    stopTrace();
    document.querySelectorAll('[data-filter]').forEach((item) => {
      const active = item === button;
      item.classList.toggle('active', active);
      item.setAttribute('aria-pressed', String(active));
    });
    const filter = button.dataset.filter;
    const visible = findings.filter((finding) => filter === 'all' || finding.filter === filter);
    if (!visible.some((finding) => finding.id === selectedFinding.id) && visible[0]) {
      selectFinding(visible[0].id);
    } else {
      renderFindingList(filter);
    }
  });
});

traceScrubber.addEventListener('input', () => {
  stopTrace();
  traceStep = Number(traceScrubber.value);
  updateTraceStep();
});

document.getElementById('trace-reset').addEventListener('click', () => {
  stopTrace();
  traceStep = 0;
  updateTraceStep();
});

tracePlay.addEventListener('click', () => {
  if (traceTimer) {
    stopTrace();
    return;
  }
  if (traceStep >= selectedFinding.nodes.length - 1) traceStep = 0;
  tracePlay.setAttribute('aria-label', 'Pause trace');
  tracePlay.innerHTML = '<i data-lucide="pause" aria-hidden="true"></i>';
  refreshIcons();
  updateTraceStep();
  traceTimer = window.setInterval(() => {
    if (traceStep >= selectedFinding.nodes.length - 1) {
      stopTrace();
      return;
    }
    traceStep += 1;
    updateTraceStep();
  }, reducedMotion ? 1100 : 700);
});

selectFinding('A');
selectTraceView('graph');
let compactGraph = window.innerWidth < 600;
window.addEventListener('resize', () => {
  const nextCompact = window.innerWidth < 600;
  if (nextCompact === compactGraph) return;
  compactGraph = nextCompact;
  traceGraph.innerHTML = traceGraphMarkup(selectedFinding);
  updateTraceStep();
});

const stages = {
  exex: {
    kicker: 'INSIDE THE NODE', title: 'Tap execution where it commits.',
    copy: 'The Execution Extension runs inside op-reth and streams each committed block—header, transactions, call traces and logs—over gRPC on a Unix socket.',
    output: 'nodestream.proto',
    boundary: 'Public ExEx is compile-verified against unmodified upstream reth; runtime and production parity are not claimed.'
  },
  tank: {
    kicker: 'INGEST + HEAL + DERIVE', title: 'Preserve the raw. Derive the useful.',
    copy: 'tank-engine consumes the stream, writes ClickHouse, backfills and heals gaps, and matches logs into derived datasets. The separate MEV detector scans fully ingested blocks in chunks.',
    output: 'world_* raw tables + api_* views',
    boundary: 'Derived MEV rows trail the chain head and remain candidates; they are not proof of intent or guaranteed realized value.'
  },
  store: {
    kicker: 'OPERATOR-CONTROLLED DATA', title: 'Keep the evidence queryable.',
    copy: 'Blocks, transactions, call traces and logs remain available in ClickHouse alongside derived token, DeFi, DEX and MEV datasets. Operators can inspect the underlying records with SQL.',
    output: 'ClickHouse + Valkey',
    boundary: 'The published quick-start schema creates empty tables. Materialized views that populate every derived table are not all published.'
  },
  api: {
    kicker: 'PROGRAMMABLE ACCESS', title: 'Serve findings where agents can use them.',
    copy: 'The Go gateway exposes REST /v1 routes with API-key tiers and rate limits. The API and Rust edge services provide WebSocket access for recent derived activity.',
    output: 'REST /v1/* + WebSocket /ws',
    boundary: 'Known limits are public, including Valkey fail-open behavior in cmd/api and the unauthenticated edge WebSocket. Do not treat them as hardened public endpoints.'
  },
  sgip: {
    kicker: 'INVESTIGATION SURFACE', title: 'Move from a row to its execution graph.',
    copy: 'SGIP renders transaction call-trace graphs plus contract and wallet views from indexed data. A transaction hash connects the derived lead to canonical trace context.',
    output: '/api/tx/{tx_hash}',
    boundary: 'Stored contract CFGs can be rendered when present, but the decompiler that produces them is not part of the public repository.'
  }
};
const pipeline = document.querySelector('.pipeline');
const stageButtons = [...document.querySelectorAll('[data-stage]')];

function selectStage(key, focus = false) {
  const stage = stages[key];
  if (!stage) return;
  stageButtons.forEach((button) => {
    const active = button.dataset.stage === key;
    button.classList.toggle('active', active);
    button.setAttribute('aria-selected', String(active));
    button.tabIndex = active ? 0 : -1;
    if (active && focus) button.focus();
  });
  document.getElementById('stage-kicker').textContent = stage.kicker;
  document.getElementById('stage-title').textContent = stage.title;
  document.getElementById('stage-copy').textContent = stage.copy;
  document.getElementById('stage-output').textContent = stage.output;
  document.getElementById('stage-boundary').textContent = stage.boundary;
  document.getElementById('stage-detail').setAttribute('aria-labelledby', `stage-${key}`);
  if (!reducedMotion) gsap.fromTo('#stage-detail > div', { opacity: 0.55, y: 5 }, { opacity: 1, y: 0, duration: 0.2, ease: 'power2.out', overwrite: true });
}

stageButtons.forEach((button) => button.addEventListener('click', () => selectStage(button.dataset.stage)));
pipeline.addEventListener('keydown', (event) => {
  if (!['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const current = stageButtons.findIndex((button) => button.getAttribute('aria-selected') === 'true');
  let next = current;
  if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = stageButtons.length - 1;
  else if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (current + 1) % stageButtons.length;
  else next = (current - 1 + stageButtons.length) % stageButtons.length;
  selectStage(stageButtons[next].dataset.stage, true);
});
selectStage('exex');

initBlueprints();
initCoreShowcase({ refreshIcons, reducedMotion });
initStory({ reducedMotion });

// Mobile navigation has click, backdrop and Escape close paths.
const menuToggle = document.getElementById('menu-toggle');
const mobileMenu = document.getElementById('mobile-menu');
const drawerBackdrop = document.getElementById('drawer-backdrop');
function openMenu() {
  mobileMenu.hidden = false;
  drawerBackdrop.hidden = false;
  menuToggle.setAttribute('aria-expanded', 'true');
  document.body.classList.add('overlay-open');
  document.getElementById('menu-close').focus();
}
function closeMenu(returnFocus = true) {
  mobileMenu.hidden = true;
  drawerBackdrop.hidden = true;
  menuToggle.setAttribute('aria-expanded', 'false');
  document.body.classList.remove('overlay-open');
  if (returnFocus) menuToggle.focus();
}
menuToggle.addEventListener('click', openMenu);
document.getElementById('menu-close').addEventListener('click', () => closeMenu());
drawerBackdrop.addEventListener('click', () => closeMenu());
mobileMenu.querySelectorAll('a').forEach((link) => link.addEventListener('click', () => closeMenu(false)));

const siteHeader = document.getElementById('site-header');
const updateHeader = () => siteHeader.classList.toggle('scrolled', window.scrollY > 24);
window.addEventListener('scroll', updateHeader, { passive: true });
updateHeader();

initPitch({
  refreshIcons,
  reducedMotion,
  onOpen: () => { if (!mobileMenu.hidden) closeMenu(false); }
});

// The presentation module owns its keyboard controls; this handler is only for navigation.
document.addEventListener('keydown', (event) => {
  if (!mobileMenu.hidden && event.key === 'Escape') closeMenu();
});

// One bounded scroll value drives the hero sculpture; no competing wheel handlers.
gsap.registerPlugin(ScrollTrigger);
try {
  const executionScene = createExecutionScene(document.getElementById('execution-scene'));
  if (!reducedMotion) {
    ScrollTrigger.create({
      trigger: '.hero', start: 'top top', end: 'bottom top', scrub: 0.5,
      onUpdate: (self) => executionScene.setProgress(self.progress)
    });
  } else {
    executionScene.setProgress(0.2);
  }
} catch (error) {
  // The static CSS/SVG poster remains visible if WebGL cannot initialize.
  document.getElementById('execution-scene').hidden = true;
}
