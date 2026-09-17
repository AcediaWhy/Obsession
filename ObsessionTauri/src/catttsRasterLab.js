// Independent raster study. Prepared pose atlases keep generation/contour work
// outside the render loop. This file is not imported by the application.
async function rasterMotionLab() {
  const $ = selector => document.querySelector(selector);
  const canvas = $('#scene'), ctx = canvas.getContext('2d', { alpha: false });
  const stage = document.createElement('canvas');
  stage.width = stage.height = 1024;
  const painter = stage.getContext('2d', { alpha: false });
  const abort = new AbortController();
  const on = (el, event, fn) => el.addEventListener(event, fn, { signal: abort.signal });
  const folder = '/lab-assets/cattts-raster-v1/';
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const layout = await fetch(`${folder}layout.json`).then(r => { if (!r.ok) throw new Error('Не удалось загрузить разметку'); return r.json(); });
  const images = {};
  await Promise.all(['original', 'leaf', 'leaf-backing', 'ear-poses', 'paw-poses'].map(async name => {
    const img = new Image();
    img.src = `${folder}${name}.webp`;
    await img.decode();
    images[name] = img;
  }));
  let running = !reduced.matches, comparing = false, view = 'all';
  let time = 0, previous = null, handle = 0, disposed = false;
  let manual = null, action = null, selected = 'ear';
  const crops = { all: [0, 0, 1024], ear: [373, 234, 234], paw: [435, 541, 208], leaf: [634, 181, 390] };
  const smooth = t => { const v = Math.max(0, Math.min(1, t)); return v * v * (3 - 2 * v); };
  const gesture = (t, start, inTime, hold, outTime) => {
    const local = t - start;
    if (local < 0 || local > inTime + hold + outTime) return 0;
    if (local < inTime) return smooth(local / inTime);
    if (local < inTime + hold) return 1;
    return 1 - smooth((local - inTime - hold) / outTime);
  };
  const state = () => {
    if (manual !== null) return { ear: selected === 'leaf' ? 0 : manual, paw: selected === 'leaf' ? 0 : manual, leaf: manual };
    if (action) {
      const p = gesture(time, action.start + .15, .65, .32, 1.05);
      return { ear: action.name === 'ear' ? p : 0, paw: action.name === 'paw' ? p : 0, leaf: action.name === 'leaf' ? p : 0 };
    }
    const t = time % 19;
    return {
      ear: gesture(t, .65, .45, .2, .8) + .38 * gesture(t, 10.2, .22, .06, .48),
      paw: gesture(t, 3.15, .85, .45, 1.5) + .55 * gesture(t, 14.2, .9, .4, 1.6),
      leaf: Math.sin(time * .55) * .5 + Math.sin(time * .22) * .24,
    };
  };
  const draw = () => {
    const started = performance.now();
    const pose = state();
    painter.setTransform(1, 0, 0, 1, 0, 0);
    painter.drawImage(images.original, 0, 0);
    if (!comparing) {
      const leaf = layout.leaf;
      if (Math.abs(pose.leaf) > .0001) {
        painter.drawImage(images['leaf-backing'], layout.leafBacking.x, layout.leafBacking.y);
        painter.save();
        painter.translate(1024, 414);
        painter.rotate(pose.leaf * .013);
        painter.translate(-1024, -414);
        painter.drawImage(images.leaf, leaf.x, leaf.y);
        painter.restore();
      }
      for (const part of layout.parts) {
        const frame = Math.round(Math.max(0, Math.min(1, pose[part.name])) * (part.frames - 1));
        if (frame) painter.drawImage(images[`${part.name}-poses`], frame % part.columns * part.w, Math.floor(frame / part.columns) * part.h, part.w, part.h, part.x, part.y, part.w, part.h);
        canvas.dataset[part.name] = String(frame);
      }
    }
    const [x, y, size] = crops[view];
    ctx.drawImage(stage, x, y, size, size, 0, 0, 1024, 1024);
    canvas.dataset.time = time.toFixed(3);
    canvas.dataset.leaf = pose.leaf.toFixed(3);
    canvas.dataset.renderMs = (performance.now() - started).toFixed(2);
  };
  const update = () => {
    $('#play').textContent = running ? 'Пауза' : 'Продолжить';
    $('#play').setAttribute('aria-pressed', String(running));
    $('#compare').textContent = comparing ? 'Вернуть анимацию' : 'Сравнить с исходником';
    $('#compare').setAttribute('aria-pressed', String(comparing));
    $('#view-badge').textContent = comparing ? 'Исходный рисунок' : manual !== null ? `Положение · ${Math.round(manual * 100)}%` : running ? 'Растровая анимация' : 'Пауза';
    canvas.dataset.running = String(running);
    canvas.dataset.comparing = String(comparing);
    canvas.dataset.view = view;
    $('#status').textContent = comparing ? 'Показан неизменённый исходник' : manual !== null ? 'Ручной просмотр · движение остановлено' : running ? 'Ухо → лапка · ветер проходит по листу' : 'Сцена остановлена';
  };
  const tick = now => {
    handle = 0;
    if (disposed || document.hidden || !running || comparing) { previous = null; return; }
    if (previous !== null) time += Math.min((now - previous) / 1000, .1);
    previous = now;
    if (action && time - action.start > 2.8) {
      running = false;
      update();
    }
    draw();
    if (running) handle = requestAnimationFrame(tick);
  };
  const schedule = () => {
    cancelAnimationFrame(handle);
    previous = null;
    handle = 0;
    update();
    draw();
    if (running && !comparing && !document.hidden) handle = requestAnimationFrame(tick);
  };
  on($('#play'), 'click', () => {
    if (!running && (manual !== null || action)) { manual = null; action = null; time = 0; $('#pose').value = '0'; $('#pose-value').textContent = '0%'; }
    running = !running;
    comparing = false;
    schedule();
  });
  on($('#replay'), 'click', () => { time = 0; manual = action = null; running = true; comparing = false; $('#pose').value = '0'; $('#pose-value').textContent = '0%'; schedule(); });
  on($('#compare'), 'click', () => { comparing = !comparing; schedule(); });
  on($('#pose'), 'input', event => { manual = Number(event.target.value) / 100; action = null; running = false; comparing = false; $('#pose-value').textContent = `${event.target.value}%`; schedule(); });
  for (const button of document.querySelectorAll('[data-view]')) on(button, 'click', () => {
    view = button.dataset.view;
    if (view !== 'all') selected = view;
    for (const other of document.querySelectorAll('[data-view]')) other.setAttribute('aria-pressed', String(other === button));
    update(); draw();
  });
  for (const button of document.querySelectorAll('[data-action]')) on(button, 'click', () => {
    selected = button.dataset.action; action = { name: selected, start: 0 }; manual = null; time = 0; running = true; comparing = false;
    $('#pose').value = '0'; $('#pose-value').textContent = '0%';
    schedule();
  });
  on(document, 'visibilitychange', schedule);
  on(reduced, 'change', () => { if (reduced.matches) { running = false; schedule(); } });
  const dispose = () => { disposed = true; cancelAnimationFrame(handle); abort.abort(); };
  on(window, 'pagehide', dispose);
  if (import.meta.hot) import.meta.hot.dispose(dispose);
  for (const element of document.querySelectorAll('[disabled]')) element.disabled = false;
  $('#loading').hidden = true;
  canvas.dataset.ready = 'true';
  schedule();
}

rasterMotionLab().catch(error => {
  console.error(error);
  document.querySelector('#loading').textContent = `Не удалось открыть пробу: ${error.message}`;
  document.querySelector('#status').textContent = 'Ошибка загрузки';
});
