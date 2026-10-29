import { createCoreScene } from './core-scene.js';

const coreComponents = {
  stack: {
    exex: {
      index: '01', kicker: 'LAYER 01 / EXTRACTION', title: 'Reth Execution Extension',
      copy: 'Committed execution leaves the node as a typed stream of blocks, transactions, logs and call traces over gRPC on a Unix socket.',
      surface: 'exex/ + nodestream.proto', label: 'Reth ExEx'
    },
    tank: {
      index: '02', kicker: 'LAYER 02 / INGEST + DERIVE', title: 'tank-engine + MEV detector',
      copy: 'The ingestion layer writes execution records, heals gaps and derives useful datasets. The separate MEV detector scans fully ingested blocks in chunks.',
      surface: 'tank-engine/ + mev-detector/', label: 'tank-engine'
    },
    store: {
      index: '03', kicker: 'LAYER 03 / OPERATOR DATA', title: 'ClickHouse + Valkey',
      copy: 'Raw blocks, transactions, traces and logs remain queryable beside derived token, DeFi, DEX and MEV views. The operator retains the underlying evidence.',
      surface: 'schema/ + ClickHouse + Valkey', label: 'ClickHouse'
    },
    api: {
      index: '04', kicker: 'LAYER 04 / PROGRAMMABLE ACCESS', title: 'REST + WebSocket APIs',
      copy: 'The Go API exposes derived findings and execution data to applications and agents. WebSocket surfaces provide recent activity without hiding the REST investigation path.',
      surface: 'cmd/api/ + /v1/* + /ws', label: 'API layer'
    },
    sgip: {
      index: '05', kicker: 'LAYER 05 / INVESTIGATION', title: 'SGIP investigation graph',
      copy: 'SGIP turns a transaction hash into an inspectable call graph with transaction context, connecting a derived lead to canonical execution evidence.',
      surface: 'sgip/ + /api/tx/{tx_hash}', label: 'SGIP'
    }
  },
  transaction: {
    block: {
      index: '01', kicker: 'CONTEXT / COMMITTED BLOCK', title: 'Block context',
      copy: 'The outer frame anchors the transaction to a committed block. It supplies chain context without implying that the model is a live block or a production query.',
      surface: 'world_blocks + block metadata', label: 'Block'
    },
    transaction: {
      index: '02', kicker: 'IDENTITY / EXACT TRANSACTION', title: 'Transaction envelope',
      copy: 'The transaction hash is the shared identifier across raw records, derived findings, REST responses and the SGIP investigation graph.',
      surface: 'world_transactions + tx_hash', label: 'Transaction'
    },
    trace: {
      index: '03', kicker: 'EXECUTION / CANONICAL CALLS', title: 'Call-trace anatomy',
      copy: 'The nested route represents canonical call relationships. Investigators can inspect how execution unfolded instead of treating a derived row as self-explanatory.',
      surface: 'world_traces + SGIP graph elements', label: 'Call trace'
    },
    finding: {
      index: '04', kicker: 'DERIVATION / INVESTIGATION LEAD', title: 'Derived finding',
      copy: 'A derived MEV row can expose a useful lead and its transaction hash. It remains a candidate—not proof of intent, guaranteed classification or realized profit.',
      surface: 'world_mev_activity + /v1/mev/analytics', label: 'Derived lead'
    }
  }
};

export function initCoreShowcase({ refreshIcons, reducedMotion }) {
  const container = document.getElementById('core-scene');
  const stage = container.closest('.core-stage');
  const modeTabs = [...document.querySelectorAll('[data-core-mode]')];
  const explodeButton = document.getElementById('core-explode');
  const motionButton = document.getElementById('core-motion');
  const resetButton = document.getElementById('core-reset');
  const componentList = document.getElementById('core-component-list');
  let activeMode = 'stack';
  let selectedKey = 'exex';
  let exploded = false;
  let playing = !reducedMotion;
  let scene = null;

  function updateInspector() {
    const component = coreComponents[activeMode][selectedKey];
    if (!component) return;
    document.getElementById('core-component-kicker').textContent = component.kicker;
    document.getElementById('core-component-title').textContent = component.title;
    document.getElementById('core-component-copy').textContent = component.copy;
    document.getElementById('core-component-surface').textContent = component.surface;
    componentList.innerHTML = Object.entries(coreComponents[activeMode]).map(([key, item]) => `
      <button type="button" class="core-component-button ${key === selectedKey ? 'active' : ''}" data-core-component="${key}" aria-pressed="${key === selectedKey}">
        <span>${item.index}</span><strong>${item.label}</strong>
      </button>`).join('');
  }

  function selectComponent(key) {
    if (!coreComponents[activeMode][key]) return;
    selectedKey = key;
    scene?.select(key);
    updateInspector();
  }

  function updateMotionControl() {
    motionButton.setAttribute('aria-pressed', String(playing));
    motionButton.innerHTML = `<i data-lucide="${playing ? 'pause' : 'play'}" aria-hidden="true"></i><span>${playing ? 'Pause motion' : 'Play motion'}</span>`;
    refreshIcons();
  }

  function setMode(mode, focus = false) {
    if (!coreComponents[mode]) return;
    activeMode = mode;
    modeTabs.forEach((tab) => {
      const selected = tab.dataset.coreMode === mode;
      tab.classList.toggle('active', selected);
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
      if (selected && focus) tab.focus();
    });
    document.getElementById('core-model-name').textContent = mode === 'stack' ? 'SYSTEM STACK / 5 LAYERS' : 'TRANSACTION ANATOMY / 4 LEVELS';
    scene?.setMode(mode);
    selectedKey = mode === 'stack' ? 'exex' : 'block';
    updateInspector();
  }

  modeTabs.forEach((tab) => tab.addEventListener('click', () => setMode(tab.dataset.coreMode)));
  document.querySelector('.core-model-switch').addEventListener('keydown', (event) => {
    if (!['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const current = modeTabs.findIndex((tab) => tab.dataset.coreMode === activeMode);
    let next = current;
    if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = modeTabs.length - 1;
    else if (event.key === 'ArrowRight') next = (current + 1) % modeTabs.length;
    else next = (current - 1 + modeTabs.length) % modeTabs.length;
    setMode(modeTabs[next].dataset.coreMode, true);
  });

  componentList.addEventListener('click', (event) => {
    const button = event.target.closest('[data-core-component]');
    if (button) selectComponent(button.dataset.coreComponent);
  });

  explodeButton.addEventListener('click', () => {
    exploded = !exploded;
    explodeButton.setAttribute('aria-pressed', String(exploded));
    explodeButton.querySelector('span').textContent = exploded ? 'Assemble view' : 'Explode view';
    scene?.setExploded(exploded);
  });

  motionButton.addEventListener('click', () => {
    playing = !playing;
    scene?.setPlaying(playing);
    updateMotionControl();
  });

  resetButton.addEventListener('click', () => scene?.resetCamera());

  setMode('stack');

  try {
    scene = createCoreScene(container, { onSelect: selectComponent });
    scene.setPlaying(playing);
    stage.classList.add('scene-ready');
    updateInspector();
    updateMotionControl();
  } catch (error) {
    container.hidden = true;
    stage.classList.add('scene-fallback');
    [...modeTabs, explodeButton, motionButton, resetButton].forEach((button) => { button.disabled = true; });
    document.querySelector('.core-gesture').textContent = 'Static model preview · Interactive 3D is unavailable in this browser';
    updateInspector();
  }

  return {
    getStats: () => scene?.getStats() || null,
    dispose: () => scene?.dispose()
  };
}
