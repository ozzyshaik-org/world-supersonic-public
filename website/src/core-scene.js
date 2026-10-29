import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';

const STACK = [
  { key: 'exex', label: 'RETH EXEX', width: 5.1, depth: 3.9 },
  { key: 'tank', label: 'TANK-ENGINE', width: 5.5, depth: 4.2 },
  { key: 'store', label: 'CLICKHOUSE', width: 5.9, depth: 4.5 },
  { key: 'api', label: 'API LAYER', width: 5.35, depth: 4.05 },
  { key: 'sgip', label: 'SGIP', width: 4.75, depth: 3.65 }
];
const TRANSACTION = [
  { key: 'block', label: 'BLOCK' },
  { key: 'transaction', label: 'TRANSACTION' },
  { key: 'trace', label: 'CALL TRACE' },
  { key: 'finding', label: 'DERIVED LEAD' }
];

export function createCoreScene(container, { onSelect }) {
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(0x0a1210, 0.026);
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'high-performance' });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.2;
  renderer.setClearColor(0x0a1210, 0);
  renderer.domElement.setAttribute('aria-hidden', 'true');
  container.appendChild(renderer.domElement);

  const camera = new THREE.PerspectiveCamera(38, 1, 0.1, 120);
  const defaultCamera = new THREE.Vector3(8.8, 5.5, 10.8);
  camera.position.copy(defaultCamera);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.set(0, 0, 0);
  controls.enableDamping = true;
  controls.dampingFactor = 0.065;
  controls.enablePan = false;
  controls.minDistance = 7;
  controls.maxDistance = 22;
  controls.minPolarAngle = 0.42;
  controls.maxPolarAngle = 1.72;

  scene.add(new THREE.HemisphereLight(0xe4fff2, 0x10271e, 1.8));
  const keyLight = new THREE.DirectionalLight(0xf2ffdf, 3.2);
  keyLight.position.set(5, 9, 7);
  scene.add(keyLight);
  const rimLight = new THREE.PointLight(0x65d9b6, 28, 25, 2);
  rimLight.position.set(-6, 2, -4);
  scene.add(rimLight);

  const root = new THREE.Group();
  root.rotation.y = -0.32;
  scene.add(root);
  const stackGroup = new THREE.Group();
  const transactionGroup = new THREE.Group();
  root.add(stackGroup, transactionGroup);
  transactionGroup.visible = false;

  const ground = new THREE.Mesh(
    new THREE.CircleGeometry(80, 64).rotateX(-Math.PI / 2),
    new THREE.MeshBasicMaterial({ color: 0x0d1b16 })
  );
  ground.position.y = -3.65;
  scene.add(ground);
  const groundRing = new THREE.Mesh(
    new THREE.RingGeometry(4.2, 4.23, 96).rotateX(-Math.PI / 2),
    new THREE.MeshBasicMaterial({ color: 0x315c4b, transparent: true, opacity: 0.5, side: THREE.DoubleSide })
  );
  groundRing.position.y = -3.62;
  scene.add(groundRing);
  const grid = new THREE.GridHelper(40, 40, 0x284b3d, 0x193329);
  grid.position.y = -3.6;
  grid.material.transparent = true;
  grid.material.opacity = 0.2;
  scene.add(grid);

  const interactiveMeshes = [];
  const stackLayers = [];
  const layerHeight = 0.48;
  STACK.forEach((item, index) => {
    const group = new THREE.Group();
    group.userData.key = item.key;
    const material = new THREE.MeshStandardMaterial({
      color: index === 2 ? 0x284f3d : 0x1c3a30,
      emissive: 0x07150f,
      metalness: 0.36,
      roughness: 0.3,
      transparent: true,
      opacity: 0.93
    });
    const geometry = new RoundedBoxGeometry(item.width, layerHeight, item.depth, 4, 0.09);
    const slab = new THREE.Mesh(geometry, material);
    slab.userData.key = item.key;
    group.add(slab);
    interactiveMeshes.push(slab);
    const edges = new THREE.LineSegments(
      new THREE.EdgesGeometry(geometry, 24),
      new THREE.LineBasicMaterial({ color: index === 2 ? 0x75b695 : 0x467261, transparent: true, opacity: 0.52 })
    );
    group.add(edges);

    // Small executable modules make each slab read as a living system layer.
    const moduleCount = 4 + index;
    for (let moduleIndex = 0; moduleIndex < moduleCount; moduleIndex += 1) {
      const column = moduleIndex % 3;
      const row = Math.floor(moduleIndex / 3);
      const module = new THREE.Mesh(
        new RoundedBoxGeometry(0.42, 0.13 + (moduleIndex % 2) * 0.05, 0.34, 2, 0.035),
        new THREE.MeshStandardMaterial({
          color: moduleIndex === index % moduleCount ? 0xb7ef59 : 0x5ba88b,
          emissive: moduleIndex === index % moduleCount ? 0x385d18 : 0x102b21,
          roughness: 0.36,
          metalness: 0.18
        })
      );
      module.position.set((column - 1) * 0.72, layerHeight / 2 + 0.1, (row - 0.5) * 0.62);
      group.add(module);
    }
    const port = new THREE.Mesh(
      new THREE.CylinderGeometry(0.07, 0.07, 0.35, 12).rotateZ(Math.PI / 2),
      new THREE.MeshBasicMaterial({ color: 0xb7ef59, toneMapped: false })
    );
    port.position.set(item.width / 2 + 0.15, 0, 0);
    group.add(port);
    group.position.y = (2 - index) * 0.94;
    stackGroup.add(group);
    stackLayers.push({ group, material, item, index, labelPoint: new THREE.Vector3(item.width / 2 + 0.6, 0, 0) });
  });

  const flowPulses = Array.from({ length: 5 }, (_, index) => {
    const pulse = new THREE.Mesh(
      new THREE.SphereGeometry(0.075, 10, 10),
      new THREE.MeshBasicMaterial({ color: index % 2 ? 0x58c9a9 : 0xb7ef59, toneMapped: false })
    );
    stackGroup.add(pulse);
    return pulse;
  });

  // Transaction cutaway: nested block, transaction, trace and derived-finding structures.
  const transactionParts = {};
  function makeShell(key, size, color, opacity) {
    const group = new THREE.Group();
    group.userData.key = key;
    const geometry = new RoundedBoxGeometry(size.x, size.y, size.z, 3, 0.08);
    const material = new THREE.MeshStandardMaterial({
      color, emissive: 0x07150f, metalness: 0.28, roughness: 0.25,
      transparent: true, opacity, depthWrite: false
    });
    const shell = new THREE.Mesh(geometry, material);
    shell.userData.key = key;
    group.add(shell);
    interactiveMeshes.push(shell);
    group.add(new THREE.LineSegments(
      new THREE.EdgesGeometry(geometry),
      new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.72 })
    ));
    transactionGroup.add(group);
    transactionParts[key] = { group, material };
    return group;
  }
  makeShell('block', new THREE.Vector3(5.7, 4.5, 4.3), 0x3c745d, 0.08);
  makeShell('transaction', new THREE.Vector3(4.15, 3.05, 2.9), 0x4b9678, 0.12);

  const traceGroup = new THREE.Group();
  traceGroup.userData.key = 'trace';
  transactionGroup.add(traceGroup);
  transactionParts.trace = { group: traceGroup, material: null };
  const tracePoints = Array.from({ length: 6 }, (_, index) => new THREE.Vector3(
    -1.7 + index * 0.68,
    Math.sin(index * 1.35) * 0.9,
    index % 2 ? 0.65 : -0.65
  ));
  const traceCurve = new THREE.CatmullRomCurve3(tracePoints, false, 'catmullrom', 0.25);
  const tracePath = new THREE.Mesh(
    new THREE.TubeGeometry(traceCurve, 100, 0.035, 6, false),
    new THREE.MeshBasicMaterial({ color: 0xb7ef59, toneMapped: false })
  );
  tracePath.userData.key = 'trace';
  traceGroup.add(tracePath);
  interactiveMeshes.push(tracePath);
  tracePoints.forEach((point, index) => {
    const node = new THREE.Mesh(
      new THREE.IcosahedronGeometry(index === 2 ? 0.19 : 0.13, 1),
      new THREE.MeshBasicMaterial({ color: index === 2 ? 0xb7ef59 : 0x62c4a5, toneMapped: false })
    );
    node.position.copy(point);
    node.userData.key = 'trace';
    traceGroup.add(node);
    interactiveMeshes.push(node);
  });

  const findingGroup = new THREE.Group();
  findingGroup.userData.key = 'finding';
  const findingCore = new THREE.Mesh(
    new THREE.IcosahedronGeometry(0.31, 1),
    new THREE.MeshPhysicalMaterial({ color: 0xb7ef59, emissive: 0x4c741c, metalness: 0.2, roughness: 0.2, clearcoat: 1 })
  );
  findingCore.userData.key = 'finding';
  findingGroup.add(findingCore);
  interactiveMeshes.push(findingCore);
  const findingRing = new THREE.Mesh(
    new THREE.TorusGeometry(0.52, 0.018, 8, 48),
    new THREE.MeshBasicMaterial({ color: 0xb7ef59, toneMapped: false })
  );
  findingRing.rotation.x = Math.PI / 2;
  findingGroup.add(findingRing);
  findingGroup.position.set(1.65, 1.25, 0.8);
  transactionGroup.add(findingGroup);
  transactionParts.finding = { group: findingGroup, material: findingCore.material };

  const transactionLabels = {
    block: new THREE.Vector3(-2.8, 2.2, 0),
    transaction: new THREE.Vector3(2.1, -1.5, 0),
    trace: new THREE.Vector3(-1.6, -1.35, 0),
    finding: new THREE.Vector3(0.5, 0.5, 0)
  };
  const transactionPulses = Array.from({ length: 3 }, () => {
    const pulse = new THREE.Mesh(new THREE.SphereGeometry(0.075, 10, 10), new THREE.MeshBasicMaterial({ color: 0xb7ef59, toneMapped: false }));
    traceGroup.add(pulse);
    return pulse;
  });

  let mode = 'stack';
  let exploded = false;
  let playing = !reducedMotion;
  let selectedKey = 'exex';
  let inView = true;
  let interacting = false;
  let elapsed = 0;
  const clock = new THREE.Clock();
  const labelButtons = new Map();

  function rebuildLabels() {
    labelButtons.forEach((button) => button.remove());
    labelButtons.clear();
    const items = mode === 'stack' ? STACK : TRANSACTION;
    items.forEach((item) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'core-label';
      button.dataset.coreLabel = item.key;
      button.textContent = item.label;
      button.setAttribute('aria-label', `Inspect ${item.label.toLowerCase()}`);
      button.setAttribute('aria-pressed', String(item.key === selectedKey));
      button.addEventListener('click', () => onSelect(item.key));
      container.appendChild(button);
      labelButtons.set(item.key, button);
    });
  }

  function updateLabelPositions() {
    const width = container.clientWidth;
    const height = container.clientHeight;
    const items = mode === 'stack' ? stackLayers : TRANSACTION;
    items.forEach((entry) => {
      const key = entry.item?.key || entry.key;
      const button = labelButtons.get(key);
      if (!button) return;
      let world;
      if (mode === 'stack') {
        world = entry.labelPoint.clone();
        entry.group.localToWorld(world);
      } else if (key === 'finding') {
        world = transactionLabels.finding.clone();
        transactionParts.finding.group.localToWorld(world);
      } else {
        world = transactionLabels[key].clone();
        transactionParts[key].group.localToWorld(world);
      }
      const projected = world.project(camera);
      const visible = projected.z < 1 && projected.x > -1.15 && projected.x < 1.15 && projected.y > -1.15 && projected.y < 1.15;
      button.hidden = !visible;
      if (visible) {
        const halfWidth = button.offsetWidth / 2;
        const halfHeight = button.offsetHeight / 2;
        const projectedX = (projected.x * 0.5 + 0.5) * width;
        const projectedY = (-projected.y * 0.5 + 0.5) * height;
        const x = THREE.MathUtils.clamp(projectedX, halfWidth + 8, width - halfWidth - 8);
        const y = THREE.MathUtils.clamp(projectedY, halfHeight + 8, height - halfHeight - 8);
        button.style.transform = `translate3d(${x}px, ${y}px, 0) translate(-50%, -50%)`;
      }
    });
  }

  function updateSelection() {
    stackLayers.forEach(({ material, item }) => {
      const selected = item.key === selectedKey;
      material.emissive.setHex(selected ? 0x35531a : 0x07150f);
      material.emissiveIntensity = selected ? 0.9 : 0.45;
    });
    Object.entries(transactionParts).forEach(([key, part]) => {
      if (part.material) {
        part.material.emissive?.setHex(key === selectedKey ? 0x527c20 : 0x07150f);
        part.material.emissiveIntensity = key === selectedKey ? 1 : 0.45;
      }
    });
    labelButtons.forEach((button, key) => {
      button.classList.toggle('active', key === selectedKey);
      button.setAttribute('aria-pressed', String(key === selectedKey));
    });
  }

  function updateParts(delta) {
    const blend = reducedMotion ? 1 : Math.min(delta * 5.5, 1);
    stackLayers.forEach(({ group, index }) => {
      const targetY = (2 - index) * (exploded ? 1.42 : 0.94);
      const targetX = exploded ? (index - 2) * 0.28 : 0;
      group.position.y = THREE.MathUtils.lerp(group.position.y, targetY, blend);
      group.position.x = THREE.MathUtils.lerp(group.position.x, targetX, blend);
    });
    const targets = exploded ? {
      block: new THREE.Vector3(-1.9, 0.2, 0),
      transaction: new THREE.Vector3(0, 0, 0),
      trace: new THREE.Vector3(1.7, -0.15, 0),
      finding: new THREE.Vector3(3.05, 1.45, 0.8)
    } : {
      block: new THREE.Vector3(0, 0, 0),
      transaction: new THREE.Vector3(0, 0, 0),
      trace: new THREE.Vector3(0, 0, 0),
      finding: new THREE.Vector3(1.65, 1.25, 0.8)
    };
    Object.entries(transactionParts).forEach(([key, part]) => {
      part.group.position.lerp(targets[key], blend);
    });
  }

  function draw() {
    controls.update();
    renderer.render(scene, camera);
    updateLabelPositions();
  }

  function resize() {
    const width = Math.max(container.clientWidth, 1);
    const height = Math.max(container.clientHeight, 1);
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    draw();
  }
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(container);
  const intersectionObserver = new IntersectionObserver(([entry]) => {
    inView = entry.isIntersecting;
    if (inView) { clock.getDelta(); draw(); }
  }, { rootMargin: '120px' });
  intersectionObserver.observe(container);
  controls.addEventListener('start', () => { interacting = true; });
  controls.addEventListener('end', () => { interacting = false; });

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let pointerStart = null;
  renderer.domElement.addEventListener('pointerdown', (event) => { pointerStart = { x: event.clientX, y: event.clientY }; });
  renderer.domElement.addEventListener('pointerup', (event) => {
    if (!pointerStart || Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y) > 6) return;
    const bounds = renderer.domElement.getBoundingClientRect();
    pointer.set(((event.clientX - bounds.left) / bounds.width) * 2 - 1, -((event.clientY - bounds.top) / bounds.height) * 2 + 1);
    raycaster.setFromCamera(pointer, camera);
    const visibleMeshes = interactiveMeshes.filter((mesh) => {
      let parent = mesh;
      while (parent) {
        if (!parent.visible) return false;
        parent = parent.parent;
      }
      return true;
    });
    const hit = raycaster.intersectObjects(visibleMeshes, false)[0];
    if (hit?.object.userData.key) onSelect(hit.object.userData.key);
    pointerStart = null;
  });

  renderer.setAnimationLoop(() => {
    const delta = Math.min(clock.getDelta(), 0.08);
    if (!inView || document.hidden) return;
    if (playing) elapsed += delta;
    if (playing && !interacting && !reducedMotion) root.rotation.y += delta * 0.055;
    updateParts(delta);
    flowPulses.forEach((pulse, index) => {
      const progress = (elapsed * 0.16 + index / flowPulses.length) % 1;
      pulse.position.set(0, 2.65 - progress * 5.3, 0);
      pulse.scale.setScalar(0.7 + Math.sin(progress * Math.PI) * 0.55);
    });
    transactionPulses.forEach((pulse, index) => {
      pulse.position.copy(traceCurve.getPointAt((elapsed * 0.13 + index / transactionPulses.length) % 1));
    });
    findingRing.rotation.z = elapsed * 0.35;
    draw();
  });

  rebuildLabels();
  updateSelection();
  resize();
  container.classList.add('ready');
  window.__CORE_SCENE_READY__ = true;
  window.__CORE_SCENE_STATS__ = () => ({
    calls: renderer.info.render.calls,
    triangles: renderer.info.render.triangles,
    frame: renderer.info.render.frame,
    geometries: renderer.info.memory.geometries,
    mode,
    exploded,
    selectedKey,
    playing,
    inView,
    elapsed,
    stackVisible: stackGroup.visible,
    transactionVisible: transactionGroup.visible,
    camera: camera.position.toArray(),
    rootRotation: root.rotation.y
  });

  return {
    setMode(nextMode) {
      if (!['stack', 'transaction'].includes(nextMode)) return;
      mode = nextMode;
      stackGroup.visible = mode === 'stack';
      transactionGroup.visible = mode === 'transaction';
      selectedKey = mode === 'stack' ? 'exex' : 'block';
      root.rotation.y = mode === 'stack' ? -0.32 : -0.48;
      rebuildLabels();
      updateSelection();
      draw();
    },
    setExploded(value) { exploded = Boolean(value); },
    setPlaying(value) { playing = Boolean(value); },
    select(key) { selectedKey = key; updateSelection(); },
    resetCamera() {
      // Flush residual orbit damping before restoring the exact saved view.
      const damping = controls.enableDamping;
      controls.enableDamping = false;
      controls.update();
      camera.position.copy(defaultCamera);
      controls.target.set(0, 0, 0);
      root.rotation.y = mode === 'stack' ? -0.32 : -0.48;
      controls.update();
      controls.enableDamping = damping;
      draw();
    },
    getStats() { return window.__CORE_SCENE_STATS__(); },
    dispose() {
      renderer.setAnimationLoop(null);
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
      controls.dispose();
      scene.traverse((object) => {
        object.geometry?.dispose();
        if (Array.isArray(object.material)) object.material.forEach((material) => material.dispose());
        else object.material?.dispose();
      });
      renderer.dispose();
    }
  };
}
