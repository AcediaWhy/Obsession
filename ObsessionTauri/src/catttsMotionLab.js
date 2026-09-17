const pause = document.querySelector('#pause');
const original = document.querySelector('#original');
const blink = document.querySelector('#blink');
const wind = document.querySelector('#wind');
const eyes = [...document.querySelectorAll('.cat-eye')];
pause.addEventListener('click', () => {
  const paused = document.body.dataset.paused !== 'true';
  document.body.dataset.paused = String(paused);
  pause.setAttribute('aria-pressed', String(paused));
  pause.textContent = paused ? 'Продолжить' : 'Пауза';
  blink.disabled = paused || original.checked;
});
original.addEventListener('change', () => {
  document.body.dataset.original = String(original.checked);
  blink.disabled = original.checked || document.body.dataset.paused === 'true';
});
wind.addEventListener('input', () => {
  document.documentElement.style.setProperty('--wind', wind.value);
  document.querySelector('#wind-value').textContent = `${Number(wind.value)}×`;
});
blink.addEventListener('click', () => {
  for (const eye of eyes) eye.classList.remove('manual-blink');
  void document.body.offsetWidth;
  for (const eye of eyes) eye.classList.add('manual-blink');
});
for (const eye of eyes) {
  eye.addEventListener('animationend', () => eye.classList.remove('manual-blink'));
}
document.addEventListener('visibilitychange', () => {
  document.documentElement.style.setProperty('--visibility-motion', document.hidden ? 'paused' : 'running');
});
