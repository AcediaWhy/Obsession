import { createGoldenMeadow } from './labs/goldenMeadowScene.js';
import { setupMeadowPanels } from './labs/goldenMeadowPanels.js';
import { createMeadowInteraction } from './labs/goldenMeadowInteraction.js';

function startGoldenMeadowLab() {
  const $ = selector => document.querySelector(selector);
  const canvas = $('#meadow');
  const abort = new AbortController();
  const on = (el, event, action) => el.addEventListener(event, action, { signal: abort.signal });
  const scene = createGoldenMeadow(canvas);
  setupMeadowPanels(canvas, abort.signal);
  const interaction = createMeadowInteraction();
  const stage = canvas.closest('.canvas-wrap');
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  let playing = !reduced.matches, time = 0, windTime = 0, previous = null, raf = 0, blinkAt = -20, tailAt = -20, earAt = -20, disposed = false;
  let petAt = -20, petCount = 0, stroke = null;
  const paint = () => {
    const tailPhase = (time - tailAt) / 3;
    const tailBoost = tailPhase >= 0 && tailPhase <= 1 ? Math.sin(Math.PI * tailPhase) ** 2 : 0;
    const response = interaction.sample(time);
    scene.render(time, { wind: Number($('#wind').value), windTime, earTime: time - earAt, blinkTime: time - blinkAt, tailBoost, petTime: time - petAt, ...response });
    canvas.dataset.attentionCount = String(response.attentionCount);
    canvas.dataset.gustCount = String(response.gustCount);
    canvas.dataset.playing = String(playing);
    canvas.dataset.petCount = String(petCount);
  };
  const update = () => {
    $('#pause').textContent = playing ? 'Пауза' : 'Продолжить';
    $('#pause').setAttribute('aria-pressed', String(!playing));
    $('#state').textContent = playing ? 'Ушки · хвост · ветер в поле' : 'Движение остановлено';
  };
  const tick = now => {
    raf = 0;
    if (disposed || !playing || document.hidden) { previous = null; return; }
    if (previous !== null) {
      const delta = Math.min(.08, (now - previous) / 1000);
      time += delta; windTime += delta * Number($('#wind').value);
    }
    previous = now; paint(); raf = requestAnimationFrame(tick);
  };
  const schedule = () => {
    interaction.reset();
    cancelAnimationFrame(raf); previous = null; raf = 0; update(); paint();
    if (playing && !document.hidden && !disposed) raf = requestAnimationFrame(tick);
  };
  const petCat = () => {
    // Complete the current response instead of restarting its eyelid curve.
    if (!playing || document.hidden || disposed || time - petAt < 2.9) return;
    petAt = time; petCount++; paint();
  };
  on(stage, 'pointermove', event => {
    if (!event.isPrimary || event.pointerType === 'touch' || !playing || document.hidden || disposed) return;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    interaction.move((event.clientX - rect.left) * scene.dimensions.width / rect.width,
      (event.clientY - rect.top) * scene.dimensions.height / rect.height, time, event.pointerId);
  });
  on(stage, 'pointerleave', () => interaction.reset());
  on(stage, 'pointercancel', () => interaction.reset());
  on(window, 'blur', () => interaction.reset());
  const resetStroke = () => { stroke = null; delete canvas.dataset.petHover; };
  const headPoint = event => {
    if (document.elementFromPoint(event.clientX, event.clientY) !== canvas) return null;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return null;
    const x = (event.clientX - rect.left) * scene.dimensions.width / rect.width;
    const y = (event.clientY - rect.top) * scene.dimensions.height / rect.height;
    // Forehead and cheeks, inset from the ears and silhouette edges.
    return ((x - 349) / 66) ** 2 + ((y - 223) / 49) ** 2 <= 1 ? { x, y } : null;
  };
  on(canvas, 'pointermove', event => {
    if (!event.isPrimary || !playing) { resetStroke(); return; }
    const p = headPoint(event);
    if (!p) { resetStroke(); return; }
    canvas.dataset.petHover = 'true';
    const now = performance.now();
    if (!stroke || stroke.pointerId !== event.pointerId || now - stroke.at > 220) {
      stroke = { ...p, at: now, distance: 0, pointerId: event.pointerId }; return;
    }
    const distance = Math.hypot(p.x - stroke.x, p.y - stroke.y);
    const speed = distance / Math.max(8, now - stroke.at) * 1000;
    const accumulated = distance > 55 || speed > 1200 ? 0 : stroke.distance + distance;
    stroke = { ...p, at: now, distance: accumulated, pointerId: event.pointerId };
    if (accumulated >= 24) { petCat(); stroke.distance = 0; }
  });
  on(canvas, 'pointerleave', resetStroke);
  on(canvas, 'pointercancel', resetStroke);
  on(canvas, 'blur', resetStroke);
  on(canvas, 'keydown', event => {
    if ((event.key === 'Enter' || event.key === ' ') && !event.repeat) {
      event.preventDefault(); petCat();
    }
  });
  on($('#pause'), 'click', () => { playing = !playing; schedule(); });
  on($('#blink'), 'click', () => { blinkAt = time; playing = true; schedule(); });
  on($('#tail'), 'click', () => { if (time - tailAt >= 3) tailAt = time; playing = true; schedule(); });
  on($('#ears'), 'click', () => { if (time - earAt >= 1.7) earAt = time; playing = true; schedule(); });
  on($('#wind'), 'input', () => { $('#wind-value').textContent = `${Number($('#wind').value).toFixed(1)}×`; paint(); });
  on($('#compare'), 'click', () => {
    const enabled = $('#compare').getAttribute('aria-pressed') !== 'true';
    $('#compare').setAttribute('aria-pressed', String(enabled));
    $('#compare').textContent = enabled ? 'Убрать референс' : 'Рядом с референсом';
    $('#reference-panel').hidden = !enabled; $('.scene-grid').classList.toggle('comparison', enabled);
  });
  on(document, 'visibilitychange', schedule);
  on(reduced, 'change', () => { if (reduced.matches) { playing = false; schedule(); } });
  const dispose = () => { disposed = true; cancelAnimationFrame(raf); abort.abort(); scene.dispose(); };
  on(window, 'pagehide', dispose);
  if (import.meta.hot) import.meta.hot.dispose(dispose);
  $('#loading').hidden = true; canvas.dataset.ready = 'true';
  schedule();
}

startGoldenMeadowLab();
