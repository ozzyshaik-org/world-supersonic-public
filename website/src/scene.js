import * as THREE from 'three';

const SEED = 4802026;
const DURATION = 12;
function mulberry32(seed) {
  return function random() {
    seed = (seed + 0x6d2b79f5) | 0;
    let value = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    value = (value + Math.imul(value ^ (value >>> 7), 61 | value)) ^ value;
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

export function createExecutionScene(container) {
  const random = mulberry32(SEED);
  const scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(0x0a1210, 0.034);

  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    preserveDrawingBuffer: true,
    powerPreference: 'high-performance'
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.75));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.15;
  renderer.setClearColor(0x0a1210, 0);
  container.appendChild(renderer.domElement);

  const camera = new THREE.PerspectiveCamera(39, 1, 0.1, 100);
  camera.position.set(8.4, 4.7, 11.8);
  camera.lookAt(0, 0, 0);

  const root = new THREE.Group();
  root.rotation.set(0.12, -0.58, -0.08);
  scene.add(root);

  const lime = new THREE.Color(0xb7ef59);
  const mint = new THREE.Color(0x58c9a9);
  const dim = new THREE.Color(0x2e5b4c);
  const frameMaterial = new THREE.LineBasicMaterial({ color: dim, transparent: true, opacity: 0.48 });
  const paneMaterial = new THREE.MeshBasicMaterial({
    color: 0x18352b, transparent: true, opacity: 0.1, side: THREE.DoubleSide, depthWrite: false
  });

  // Each committed-block frame is generated from the same layer mapping.
  const layerCount = 6;
  const layerDepth = 1.65;
  const layerZ = (index) => (index - (layerCount - 1) / 2) * layerDepth;
  const frames = [];
  for (let index = 0; index < layerCount; index += 1) {
    const frame = new THREE.Group();
    const taper = 1 - Math.abs(index - 2.5) * 0.025;
    const geometry = new THREE.BoxGeometry(6.2 * taper, 4.25 * taper, 0.09);
    const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), frameMaterial);
    const pane = new THREE.Mesh(geometry, paneMaterial);
    frame.add(pane, edges);
    frame.position.z = layerZ(index);
    root.add(frame);
    frames.push(frame);
  }

  // A deterministic execution path traverses the block frames.
  const pathPoints = Array.from({ length: layerCount }, (_, index) => {
    const direction = index % 2 === 0 ? -1 : 1;
    return new THREE.Vector3(
      direction * (1.2 + random() * 0.65),
      Math.sin(index * 1.35) * 1.1,
      layerZ(index)
    );
  });
  const executionCurve = new THREE.CatmullRomCurve3(pathPoints, false, 'catmullrom', 0.28);
  const pathGeometry = new THREE.TubeGeometry(executionCurve, 160, 0.035, 6, false);
  const pathMaterial = new THREE.MeshBasicMaterial({ color: lime, transparent: true, opacity: 1, toneMapped: false });
  root.add(new THREE.Mesh(pathGeometry, pathMaterial));

  const mainNodes = [];
  pathPoints.forEach((position, index) => {
    const node = new THREE.Group();
    const core = new THREE.Mesh(
      new THREE.IcosahedronGeometry(index === 2 ? 0.25 : 0.18, 1),
      new THREE.MeshBasicMaterial({ color: index === 2 ? lime : mint, toneMapped: false })
    );
    const ring = new THREE.Mesh(
      new THREE.TorusGeometry(index === 2 ? 0.44 : 0.32, 0.012, 5, 36),
      new THREE.MeshBasicMaterial({ color: index === 2 ? lime : dim, transparent: true, opacity: 0.8 })
    );
    ring.rotation.x = Math.PI / 2;
    node.add(core, ring);
    node.position.copy(position);
    root.add(node);
    mainNodes.push(node);
  });

  // Branch calls are generated as families, with seeded variation and thin connectors.
  const branchPositions = [];
  const branchVertices = [];
  pathPoints.forEach((origin, layer) => {
    const branches = layer === 2 ? 5 : 3;
    for (let branch = 0; branch < branches; branch += 1) {
      const angle = (branch / branches) * Math.PI * 2 + random() * 0.55;
      const radius = 1.15 + random() * 1.25;
      const position = new THREE.Vector3(
        Math.cos(angle) * radius,
        Math.sin(angle) * radius * 0.72,
        origin.z + (random() - 0.5) * 0.42
      );
      branchPositions.push(position);
      branchVertices.push(origin.x, origin.y, origin.z, position.x, position.y, position.z);
    }
  });
  const branchGeometry = new THREE.BufferGeometry();
  branchGeometry.setAttribute('position', new THREE.Float32BufferAttribute(branchVertices, 3));
  root.add(new THREE.LineSegments(branchGeometry, new THREE.LineBasicMaterial({ color: dim, transparent: true, opacity: 0.38 })));

  const branchMesh = new THREE.InstancedMesh(
    new THREE.OctahedronGeometry(0.075, 0),
    new THREE.MeshBasicMaterial({ color: mint, transparent: true, opacity: 0.72 }),
    branchPositions.length
  );
  const dummy = new THREE.Object3D();
  branchPositions.forEach((position, index) => {
    dummy.position.copy(position);
    const scale = 0.75 + random() * 0.75;
    dummy.scale.setScalar(scale);
    dummy.rotation.set(random() * Math.PI, random() * Math.PI, random() * Math.PI);
    dummy.updateMatrix();
    branchMesh.setMatrixAt(index, dummy.matrix);
  });
  root.add(branchMesh);

  // Distant execution particles give the field depth without becoming a second subject.
  const particlePositions = [];
  for (let index = 0; index < 180; index += 1) {
    const angle = random() * Math.PI * 2;
    const radius = 4.5 + random() * 8;
    particlePositions.push(
      Math.cos(angle) * radius,
      (random() - 0.5) * 8,
      Math.sin(angle) * radius
    );
  }
  const particleGeometry = new THREE.BufferGeometry();
  particleGeometry.setAttribute('position', new THREE.Float32BufferAttribute(particlePositions, 3));
  const particles = new THREE.Points(
    particleGeometry,
    new THREE.PointsMaterial({ color: mint, size: 0.025, transparent: true, opacity: 0.42, sizeAttenuation: true })
  );
  scene.add(particles);

  const grid = new THREE.GridHelper(34, 34, 0x244a3c, 0x173329);
  grid.position.y = -3.5;
  grid.material.transparent = true;
  grid.material.opacity = 0.24;
  scene.add(grid);

  const pulses = Array.from({ length: 3 }, () => {
    const pulse = new THREE.Mesh(
      new THREE.SphereGeometry(0.09, 12, 12),
      new THREE.MeshBasicMaterial({ color: lime, toneMapped: false })
    );
    root.add(pulse);
    return pulse;
  });

  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const clock = new THREE.Clock();
  let paused = reducedMotion;
  let inView = true;
  let simTime = 0;
  let scrollProgress = 0;
  let lastDrawnTime = -1;

  function sample(time) {
    const phase = (time / DURATION) * Math.PI * 2;
    root.rotation.y = -0.58 + Math.sin(phase) * 0.055 + scrollProgress * 0.34;
    root.rotation.x = 0.12 + Math.cos(phase) * 0.025 - scrollProgress * 0.08;
    root.position.y = Math.sin(phase) * 0.09;
    particles.rotation.y = phase;
    particles.rotation.x = Math.sin(phase) * 0.03;
    mainNodes.forEach((node, index) => {
      const scale = 1 + Math.sin(phase + index * 0.8) * 0.065;
      node.scale.setScalar(scale);
      node.children[1].rotation.z = phase + index * 0.35;
    });
    pulses.forEach((pulse, index) => {
      const position = executionCurve.getPointAt((time / DURATION + index / pulses.length) % 1);
      pulse.position.copy(position);
      pulse.scale.setScalar(0.85 + (1 + Math.sin(phase + index * 2)) * 0.2);
    });
    camera.position.x = 8.4 - scrollProgress * 1.1;
    camera.position.y = 4.7 + scrollProgress * 0.55;
    camera.lookAt(0, 0, 0);
  }

  function draw(force = false) {
    if (!force && !inView) return;
    if (!force && lastDrawnTime === simTime && paused) return;
    renderer.render(scene, camera);
    lastDrawnTime = simTime;
  }

  function resize() {
    const width = Math.max(container.clientWidth, 1);
    const height = Math.max(container.clientHeight, 1);
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    draw(true);
  }
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(container);
  const intersectionObserver = new IntersectionObserver(([entry]) => {
    inView = entry.isIntersecting;
    if (inView) { clock.getDelta(); draw(true); }
  }, { rootMargin: '100px' });
  intersectionObserver.observe(container);

  renderer.setAnimationLoop(() => {
    const delta = Math.min(clock.getDelta(), 0.08);
    if (!paused && inView && !document.hidden) {
      simTime = (simTime + delta) % DURATION;
      sample(simTime);
      draw();
    }
  });

  window.__seek = (time) => {
    const numeric = Number(time);
    paused = true;
    simTime = Number.isFinite(numeric) && numeric > 0 ? numeric : 0;
    sample(simTime);
    draw(true);
  };
  window.__play = () => {
    paused = false;
    clock.getDelta();
  };
  window.__SUPERSONIC_SCENE_STATS__ = () => ({
    calls: renderer.info.render.calls,
    triangles: renderer.info.render.triangles,
    points: renderer.info.render.points,
    geometries: renderer.info.memory.geometries,
    pixelRatio: renderer.getPixelRatio()
  });

  resize();
  sample(simTime);
  draw(true);
  container.classList.add('ready');
  window.__SCENE_READY__ = true;
  const timeParameter = new URLSearchParams(window.location.search).get('t');
  if (timeParameter !== null) window.__seek(timeParameter);

  return {
    setProgress(progress) {
      scrollProgress = THREE.MathUtils.clamp(progress, 0, 1);
      sample(simTime);
      draw(true);
    },
    dispose() {
      renderer.setAnimationLoop(null);
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
      scene.traverse((object) => {
        object.geometry?.dispose();
        if (Array.isArray(object.material)) object.material.forEach((material) => material.dispose());
        else object.material?.dispose();
      });
      renderer.dispose();
    }
  };
}
