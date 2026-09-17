import { createBlackCatRig } from './labs/blackCatRig.js';

const root = '/lab-assets/black-cat-states/';
const original = document.querySelector('#original');
const stage = document.querySelector('#rig');
const pauseButton = document.querySelector('#pause');
const neutralButton = document.querySelector('#neutral');
const strengthInput = document.querySelector('#strength');
const layersInput = document.querySelector('#layers');
const motionState = document.querySelector('#motion-state');
const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
let paused = reduced.matches;
let neutral = false;
let time = 0;
let last = 0;
let raf = 0;
let disposed = false;
let rig;

function poseOptions() {
  return { strength: Number(strengthInput.value), neutral, exploded: layersInput.checked };
}

function frame(now) {
  raf = 0;
  if (disposed || paused || neutral || document.hidden) { last = 0; return; }
  if (last) time += Math.min((now - last) / 1000, 0.05);
  last = now;
  rig.render(time, poseOptions());
  raf = requestAnimationFrame(frame);
}

function refresh() {
  if (!rig || disposed) return;
  const still = paused || neutral || document.hidden;
  const desiredSource = `${root}idle-256${still ? '-still' : ''}.webp`;
  if (original.getAttribute('src') !== desiredSource) original.src = desiredSource;
  pauseButton.textContent = paused ? 'Продолжить' : 'Пауза';
  pauseButton.setAttribute('aria-pressed', String(paused));
  neutralButton.setAttribute('aria-pressed', String(neutral));
  neutralButton.textContent = neutral ? 'Вернуть движение' : 'Сравнить стоп-кадры';
  motionState.textContent = neutral ? 'Исходная поза' : paused ? 'Пауза' : 'Плавное движение слоёв';
  rig.render(time, poseOptions());
  if (still) { cancelAnimationFrame(raf); raf = 0; last = 0; }
  else if (!raf) raf = requestAnimationFrame(frame);
}

const events = new AbortController();
const signal = events.signal;
pauseButton.addEventListener('click', () => { paused = !paused; refresh(); }, { signal });
neutralButton.addEventListener('click', () => { neutral = !neutral; refresh(); }, { signal });
document.querySelector('#blink').addEventListener('click', () => { neutral = false; paused = false; rig?.blink(time); refresh(); }, { signal });
document.querySelector('#attention').addEventListener('click', () => { neutral = false; paused = false; rig?.call(time); refresh(); }, { signal });
strengthInput.addEventListener('input', () => {
  document.querySelector('#strength-value').value = `${Number(strengthInput.value).toFixed(1).replace('.', ',')}×`;
  refresh();
}, { signal });
layersInput.addEventListener('change', refresh, { signal });
document.querySelector('#display-size').addEventListener('change', (event) => {
  document.documentElement.style.setProperty('--cat-size', `${event.target.value}px`);
}, { signal });
document.addEventListener('visibilitychange', refresh, { signal });
reduced.addEventListener('change', () => { paused = reduced.matches; refresh(); }, { signal });

async function start() {
  const image = new Image();
  image.src = `${root}idle-256-still.webp`;
  await image.decode();
  if (disposed) return;
  rig = createBlackCatRig(stage, image);
  refresh();
  const lengths = await Promise.all(['idle-256.webp', 'idle-256-still.webp'].map(async (name) => {
    const response = await fetch(`${root}${name}`);
    if (!response.ok) throw new Error(`Asset ${response.status}`);
    return (await response.arrayBuffer()).byteLength;
  }));
  if (disposed) return;
  const kb = (bytes) => `${(bytes / 1024).toFixed(1).replace('.', ',')} КиБ`;
  document.querySelector('#original-size').textContent = `${kb(lengths[0])} · анимированный WebP`;
  document.querySelector('#rig-size').textContent = `${kb(lengths[1])} · картинка; движения — кодом, без новых кадров`;
}

function dispose() {
  disposed = true;
  cancelAnimationFrame(raf);
  events.abort();
  rig?.dispose();
}

window.addEventListener('pagehide', (event) => {
  if (!event.persisted) dispose();
}, { signal });
if (import.meta.hot) import.meta.hot.dispose(dispose);
start().catch((error) => {
  if (disposed) return;
  motionState.textContent = 'Не удалось загрузить кота';
  document.querySelector('#notice').textContent = `Ошибка загрузки: ${error.message}`;
});
