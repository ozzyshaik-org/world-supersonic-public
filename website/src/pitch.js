import { slides, PITCH_TOTAL_SECONDS } from './pitch-content.js';

const clampSlide = (value) => Math.max(0, Math.min(slides.length - 1, Number.isFinite(value) ? value : 0));
const formatTime = (seconds) => `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;

export function initPitch({ refreshIcons, reducedMotion, onOpen }) {
  const overlay = document.getElementById('pitch-overlay');
  const slideMount = document.getElementById('pitch-slide');
  const nextButton = document.getElementById('pitch-next');
  const previousButton = document.getElementById('pitch-prev');
  const autoplayButton = document.getElementById('pitch-autoplay');
  const fullscreenButton = document.getElementById('pitch-fullscreen');
  const overviewPanel = document.getElementById('pitch-overview-panel');
  const notesPanel = document.getElementById('pitch-notes-panel');
  let index = 0;
  let returnFocus = null;
  let playing = false;
  let elapsed = 0;
  let lastTick = 0;
  let timer = null;

  function setBackgroundInert(value) {
    document.querySelectorAll('main, .site-header, .site-footer').forEach((element) => { element.inert = value; });
  }

  function updateQuery(open) {
    const url = new URL(window.location.href);
    if (open) {
      url.searchParams.set('pitch', '1');
      url.searchParams.set('slide', String(index + 1));
    } else {
      url.searchParams.delete('pitch');
      url.searchParams.delete('slide');
    }
    window.history.replaceState({}, '', `${url.pathname}${url.search}${url.hash}`);
  }

  function updatePlaybackControl() {
    autoplayButton.setAttribute('aria-pressed', String(playing));
    autoplayButton.setAttribute('aria-label', playing ? 'Pause automatic pitch playback' : 'Play pitch automatically');
    autoplayButton.innerHTML = `<i data-lucide="${playing ? 'pause' : 'play'}" aria-hidden="true"></i><span>${playing ? 'Pause' : 'Auto play'}</span>`;
    refreshIcons();
  }

  function updateTiming() {
    const slide = slides[index];
    const previousSeconds = slides.slice(0, index).reduce((total, item) => total + item.duration, 0);
    const slideProgress = Math.min(1, elapsed / slide.duration);
    const totalProgress = Math.min(1, (previousSeconds + elapsed) / PITCH_TOTAL_SECONDS);
    document.getElementById('pitch-autoplay-progress').style.transform = `scaleX(${slideProgress})`;
    document.getElementById('pitch-time-progress').style.transform = `scaleX(${totalProgress})`;
    document.getElementById('pitch-timing').textContent = `${formatTime(Math.max(0, Math.ceil(slide.duration - elapsed)))} LEFT / ${formatTime(PITCH_TOTAL_SECONDS)} TOTAL`;
  }

  function stopAutoplay() {
    if (timer) window.clearInterval(timer);
    timer = null;
    playing = false;
    updatePlaybackControl();
  }

  function tick() {
    const now = performance.now();
    elapsed += (now - lastTick) / 1000;
    lastTick = now;
    if (elapsed >= slides[index].duration) {
      if (index === slides.length - 1) {
        elapsed = slides[index].duration;
        stopAutoplay();
        updateTiming();
        return;
      }
      index += 1;
      renderPitch();
      return;
    }
    updateTiming();
  }

  function startAutoplay() {
    if (playing) return;
    if (elapsed >= slides[index].duration) elapsed = 0;
    playing = true;
    lastTick = performance.now();
    timer = window.setInterval(tick, reducedMotion ? 250 : 100);
    updatePlaybackControl();
  }

  function closePanels(returnFocus = false) {
    const trigger = !notesPanel.hidden
      ? document.getElementById('pitch-notes')
      : document.getElementById('pitch-overview');
    overviewPanel.hidden = true;
    notesPanel.hidden = true;
    document.getElementById('pitch-overview').setAttribute('aria-pressed', 'false');
    document.getElementById('pitch-notes').setAttribute('aria-pressed', 'false');
    if (returnFocus) trigger.focus();
  }

  function togglePanel(name) {
    const showOverview = name === 'overview';
    const panel = showOverview ? overviewPanel : notesPanel;
    const wasHidden = panel.hidden;
    closePanels();
    panel.hidden = !wasHidden;
    document.getElementById(showOverview ? 'pitch-overview' : 'pitch-notes').setAttribute('aria-pressed', String(wasHidden));
    if (wasHidden && showOverview) {
      overviewPanel.querySelector(`[data-overview-slide="${index}"]`)?.focus();
    } else if (wasHidden) {
      document.getElementById('pitch-source').focus();
    }
  }

  function renderPitch() {
    const slide = slides[index];
    elapsed = 0;
    lastTick = performance.now();
    slideMount.innerHTML = `<div class="pitch-slide-inner pitch-two-col">
      <div class="pitch-copy"><div class="pitch-slide-state"><span>${slide.status}</span><span>${String(index + 1).padStart(2, '0')} / ${String(slides.length).padStart(2, '0')}</span></div><span class="pitch-kicker">${slide.kicker}</span><h2 id="pitch-slide-title">${slide.title}</h2><p class="pitch-lede">${slide.body}</p>${slide.meta ? `<p class="pitch-meta">${slide.meta}</p>` : ''}</div>
      <div class="pitch-visual">${slide.visual}</div>
    </div>`;
    document.getElementById('pitch-count').textContent = `${String(index + 1).padStart(2, '0')} / ${String(slides.length).padStart(2, '0')}`;
    document.getElementById('pitch-section-label').textContent = `${String(index + 1).padStart(2, '0')} / ${slide.short.toUpperCase()}`;
    document.getElementById('pitch-progress').innerHTML = slides.map((item, itemIndex) => `<button type="button" class="${itemIndex === index ? 'active' : ''}" data-slide="${itemIndex}" aria-label="Go to slide ${itemIndex + 1}: ${item.short}" ${itemIndex === index ? 'aria-current="step"' : ''}></button>`).join('');
    document.getElementById('pitch-overview-list').innerHTML = slides.map((item, itemIndex) => `<button type="button" data-overview-slide="${itemIndex}" class="${itemIndex === index ? 'active' : ''}" ${itemIndex === index ? 'aria-current="step"' : ''}><span>${String(itemIndex + 1).padStart(2, '0')}</span><strong>${item.short}</strong><small>${formatTime(item.duration)}</small></button>`).join('');
    document.getElementById('pitch-note-text').textContent = slide.notes;
    const source = document.getElementById('pitch-source');
    source.href = slide.source.url;
    source.innerHTML = `${slide.source.label} <i data-lucide="arrow-up-right" aria-hidden="true"></i>`;
    previousButton.disabled = index === 0;
    nextButton.innerHTML = `${index === slides.length - 1 ? 'Finish' : 'Next'} <i data-lucide="${index === slides.length - 1 ? 'x' : 'arrow-right'}" aria-hidden="true"></i>`;
    updateTiming();
    updateQuery(true);
    refreshIcons();
  }

  function openPitch(startIndex = 0) {
    returnFocus = document.activeElement;
    onOpen?.();
    index = clampSlide(startIndex);
    closePanels();
    overlay.hidden = false;
    document.body.classList.add('overlay-open');
    setBackgroundInert(true);
    renderPitch();
    document.getElementById('pitch-close').focus();
  }

  function closePitch(restoreFocus = true) {
    stopAutoplay();
    overlay.hidden = true;
    closePanels();
    document.body.classList.remove('overlay-open');
    setBackgroundInert(false);
    updateQuery(false);
    if (restoreFocus && returnFocus instanceof HTMLElement) returnFocus.focus();
  }

  function goToSlide(nextIndex, manual = true) {
    if (manual) stopAutoplay();
    index = clampSlide(nextIndex);
    renderPitch();
  }

  function nextPitch() {
    stopAutoplay();
    if (index === slides.length - 1) closePitch();
    else goToSlide(index + 1, false);
  }

  function updateFullscreenControl() {
    const active = Boolean(document.fullscreenElement);
    fullscreenButton.setAttribute('aria-pressed', String(active));
    fullscreenButton.setAttribute('aria-label', active ? 'Exit fullscreen' : 'Enter fullscreen');
    fullscreenButton.innerHTML = `<i data-lucide="${active ? 'minimize-2' : 'maximize-2'}" aria-hidden="true"></i><span>${active ? 'Exit full' : 'Fullscreen'}</span>`;
    refreshIcons();
  }

  document.querySelectorAll('[data-open-pitch]').forEach((button) => button.addEventListener('click', () => openPitch()));
  document.getElementById('pitch-close').addEventListener('click', () => closePitch());
  document.getElementById('pitch-overview').addEventListener('click', () => togglePanel('overview'));
  document.getElementById('pitch-notes').addEventListener('click', () => togglePanel('notes'));
  nextButton.addEventListener('click', nextPitch);
  previousButton.addEventListener('click', () => { stopAutoplay(); if (index > 0) goToSlide(index - 1, false); });
  autoplayButton.addEventListener('click', () => { if (playing) stopAutoplay(); else startAutoplay(); });

  fullscreenButton.addEventListener('click', async () => {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await overlay.requestFullscreen();
    } catch {
      fullscreenButton.disabled = true;
      fullscreenButton.title = 'Fullscreen was refused by this browser';
    }
  });
  document.addEventListener('fullscreenchange', updateFullscreenControl);
  document.addEventListener('visibilitychange', () => {
    if (document.hidden && playing) stopAutoplay();
  });
  if (!overlay.requestFullscreen || !document.exitFullscreen) {
    fullscreenButton.disabled = true;
    fullscreenButton.title = 'Fullscreen is unavailable in this browser';
  }

  document.getElementById('pitch-progress').addEventListener('click', (event) => {
    const button = event.target.closest('[data-slide]');
    if (button) goToSlide(Number(button.dataset.slide));
  });
  document.getElementById('pitch-overview-list').addEventListener('click', (event) => {
    const button = event.target.closest('[data-overview-slide]');
    if (!button) return;
    goToSlide(Number(button.dataset.overviewSlide));
    closePanels();
    document.getElementById('pitch-overview').focus();
  });
  slideMount.addEventListener('click', (event) => {
    const workspace = event.target.closest('[data-pitch-workspace]');
    if (!workspace) return;
    closePitch(false);
    document.getElementById('product').scrollIntoView({ behavior: reducedMotion ? 'instant' : 'smooth' });
    document.getElementById('product-title').setAttribute('tabindex', '-1');
    document.getElementById('product-title').focus({ preventScroll: true });
  });

  document.addEventListener('keydown', (event) => {
    if (overlay.hidden || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!overviewPanel.hidden || !notesPanel.hidden) closePanels(true);
      else closePitch();
      return;
    }
    if (event.key === 'ArrowRight' || event.key === 'PageDown') { event.preventDefault(); nextPitch(); return; }
    if (event.key === 'ArrowLeft' || event.key === 'PageUp') { event.preventDefault(); stopAutoplay(); if (index > 0) goToSlide(index - 1, false); return; }
    if (event.key === 'Home') { event.preventDefault(); goToSlide(0); return; }
    if (event.key === 'End') { event.preventDefault(); goToSlide(slides.length - 1); return; }
    if (event.key.toLowerCase() === 'o') { event.preventDefault(); togglePanel('overview'); return; }
    if (event.key.toLowerCase() === 'n') { event.preventDefault(); togglePanel('notes'); return; }
    if (event.key.toLowerCase() === 'f') { event.preventDefault(); fullscreenButton.click(); return; }
    if (event.key === ' ' && !event.target.closest('button, a')) { event.preventDefault(); if (playing) stopAutoplay(); else startAutoplay(); return; }
    if (event.key === 'Tab') {
      const focusable = [...overlay.querySelectorAll('button:not(:disabled), a[href], [tabindex="0"]')].filter((element) => element.offsetParent !== null);
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    }
  });

  updatePlaybackControl();
  updateFullscreenControl();
  const params = new URLSearchParams(window.location.search);
  if (params.get('pitch') === '1') openPitch(clampSlide(Number(params.get('slide') || 1) - 1));

  window.__PITCH_STATS__ = () => ({
    index, count: slides.length, playing, elapsed, totalSeconds: PITCH_TOTAL_SECONDS,
    overviewOpen: !overviewPanel.hidden, notesOpen: !notesPanel.hidden,
    fullscreen: Boolean(document.fullscreenElement), overlayOpen: !overlay.hidden
  });

  return { open: openPitch, close: closePitch, goToSlide };
}
