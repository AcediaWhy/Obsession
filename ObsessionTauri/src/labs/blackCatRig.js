// A small idle rig made from the existing 256 px still. Layers are cut once;
// Body/head use compositor transforms; the small tail bends from a cached cutout.
const SIZE = 256;
const FUR = '#040304';
const EYES = [[85, 98, 47, 45], [152, 98, 48, 45]];

const smooth = (value) => {
  const x = Math.max(0, Math.min(1, value));
  return x * x * (3 - 2 * x);
};

function layerCanvas(source, include) {
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = SIZE;
  const ctx = canvas.getContext('2d');
  const pixels = ctx.createImageData(SIZE, SIZE);
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const i = (y * SIZE + x) * 4;
      if (include(x, y)) pixels.data.set(source.data.subarray(i, i + 4), i);
    }
  }
  ctx.putImageData(pixels, 0, 0);
  canvas.setAttribute('aria-hidden', 'true');
  return canvas;
}

function blinkAmount(elapsed) {
  if (elapsed < 0 || elapsed > 0.24) return 0;
  return elapsed < 0.09 ? smooth(elapsed / 0.09) : 1 - smooth((elapsed - 0.09) / 0.15);
}

// Ease into a gesture, hold the pose, then release it. No snapping at loop edges.
function gesture(time, start, duration, ease = 0.55) {
  const elapsed = time - start;
  if (elapsed <= 0 || elapsed >= duration) return 0;
  return smooth(elapsed / ease) * smooth((duration - elapsed) / ease);
}

// Follow the tail/body seam. A rectangular cut also picks up the illuminated
// flank above the tail root, which becomes a detached spike when rotated.
function isTailPixel(x, y) {
  if (y < 158 || y >= 237) return false;
  const seam = y < 210 ? 78 : y < 222 ? 79 : y < 226 ? 81 : y < 230 ? 85 : 87;
  return x < seam;
}

// The lower curve/root is fixed. Displacement and its slope both reach zero
// at the join; only the free end swishes, with a little delayed follow-through.
function tailDisplacement(y, time, amount, flick) {
  const free = smooth((210 - y) / 52);
  return free * amount * (Math.sin(time * Math.PI * 2 / 3.8 - free * 0.45) * 8
    + flick * Math.sin(time * Math.PI * 2 / 1.3 - free * 0.7) * 3);
}

export function createBlackCatRig(container, image) {
  const sourceCanvas = document.createElement('canvas');
  sourceCanvas.width = sourceCanvas.height = SIZE;
  const sourceCtx = sourceCanvas.getContext('2d');
  sourceCtx.drawImage(image, 0, 0, SIZE, SIZE);
  const source = sourceCtx.getImageData(0, 0, SIZE, SIZE);

  const tail = layerCanvas(source, isTailPixel);
  const body = layerCanvas(source, (x, y) => y >= 153 && !isTailPixel(x, y));
  // Unpainted fur extends underneath the body at the pivot, not a duplicate
  // of its highlighted edge. This keeps the root joined during tail swings.
  const tailCtx = tail.getContext('2d');
  tailCtx.globalCompositeOperation = 'destination-over';
  tailCtx.fillStyle = FUR;
  tailCtx.beginPath();
  tailCtx.moveTo(76, 213);
  tailCtx.lineTo(94, 213);
  tailCtx.lineTo(97, 226);
  tailCtx.lineTo(85, 232);
  tailCtx.lineTo(76, 229);
  tailCtx.closePath();
  tailCtx.fill();
  tailCtx.globalCompositeOperation = 'source-over';
  const tailTexture = document.createElement('canvas');
  tailTexture.width = tailTexture.height = SIZE;
  tailTexture.getContext('2d').drawImage(tail, 0, 0);
  const head = layerCanvas(source, (_x, y) => y <= 160);
  const eyes = layerCanvas(source, (x, y) => EYES.some(([left, top, width, height]) => x >= left && x < left + width && y >= top && y < top + height));
  const headCtx = head.getContext('2d');
  headCtx.fillStyle = FUR;
  EYES.forEach(([x, y, width, height]) => headCtx.fillRect(x, y, width, height));

  // Hidden overlap beneath the head prevents a gap at the neck during tilts.
  const bodyCtx = body.getContext('2d');
  bodyCtx.globalCompositeOperation = 'destination-over';
  bodyCtx.fillStyle = FUR;
  bodyCtx.beginPath();
  bodyCtx.moveTo(104, 144);
  bodyCtx.lineTo(187, 144);
  bodyCtx.lineTo(201, 173);
  bodyCtx.lineTo(91, 173);
  bodyCtx.closePath();
  bodyCtx.fill();
  bodyCtx.globalCompositeOperation = 'source-over';

  const headGroup = document.createElement('div');
  tail.className = 'cat-rig-part cat-rig-tail';
  body.className = 'cat-rig-part cat-rig-body';
  headGroup.className = 'cat-rig-head-group';
  head.className = 'cat-rig-part cat-rig-head';
  eyes.className = 'cat-rig-part cat-rig-eyes';
  headGroup.append(head, eyes);
  container.classList.add('cat-rig');
  container.replaceChildren(tail, body, headGroup);

  // Tail and body share the same transform/pivot so the attachment never slides.
  body.style.transformOrigin = '58% 95.3%';
  tail.style.transformOrigin = body.style.transformOrigin;
  headGroup.style.transformOrigin = '58% 60%';
  eyes.style.transformOrigin = '50% 47.3%';
  let manualBlink = -100;
  let attention = -100;

  function render(time, { strength = 1, neutral = false, exploded = false } = {}) {
    const amount = neutral ? 0 : strength;
    const breath = (1 - Math.cos(time * Math.PI * 2 / 3.8)) * 0.5;
    const sway = Math.sin(time * Math.PI * 2 / 6.4);
    const callTime = time - attention;
    const call = callTime >= 0 && callTime < 2.4 ? Math.sin(Math.PI * callTime / 2.4) ** 2 : 0;
    const phase = time % 16;
    const direction = Math.floor(time / 16) % 2 ? -1 : 1;
    const curious = gesture(phase, 0.55, 2.7);
    const lookLeft = gesture(phase, 4, 2.3);
    const lookRight = gesture(phase, 7, 2.5);
    const stretch = gesture(phase, 10.3, 2.5, 0.8);
    const settle = gesture(phase, 13.4, 1.8, 0.65);
    const look = (lookLeft - lookRight) * direction;
    const interest = Math.max(curious, call);
    const flick = gesture(phase, 1, 2.5) + gesture(phase, 6.8, 2.5);
    const tilt = (sway * 1.2 - interest * 5.8 * direction + look * 4.5) * amount;
    const lift = (-breath * 2.5 - interest * 3 - stretch * 6 + settle * 3) * amount;
    const lean = (look * 0.7 - interest * 0.3 * direction) * amount;
    const autoTime = time % 5.8;
    const blink = neutral ? 0 : Math.max(blinkAmount(time - manualBlink), blinkAmount(autoTime - 2.6), blinkAmount(autoTime - 3.02) * 0.8);
    const spread = exploded ? 1 : 0;

    body.style.transform = `translateY(${spread * 8}%) rotate(${lean}deg) scale(${1 + (breath * 0.009 - stretch * 0.012 + settle * 0.018) * amount}, ${1 + (breath * 0.02 + stretch * 0.045 - settle * 0.025) * amount})`;
    tail.style.transform = `translateX(${-spread * 22}%) ${body.style.transform}`;
    tailCtx.clearRect(0, 0, SIZE, SIZE);
    if (!amount) tailCtx.drawImage(tailTexture, 0, 0);
    else {
      // Draw the pinned root unchanged, then bend the free portion by rows.
      // Fractional X positions preserve smooth motion without new image assets.
      tailCtx.drawImage(tailTexture, 0, 210, SIZE, 46, 0, 210, SIZE, 46);
      for (let y = 158; y < 210; y++) {
        tailCtx.drawImage(tailTexture, 0, y, SIZE, 1, tailDisplacement(y, time, amount, flick), y, SIZE, 1);
      }
    }
    headGroup.style.transform = `translate(${(sway * 0.2 + look * 1.1) * amount}%, ${lift / SIZE * 100 - spread * 14}%) rotate(${tilt}deg) scale(${1 + interest * 0.025 * amount}, ${1 + stretch * 0.014 * amount})`;
    eyes.style.transform = `translate(${look * 0.45 * amount}%, ${-spread * 12}%) scaleY(${(1 - blink * 0.965) * (1 - settle * 0.25 * amount)})`;
    container.dataset.exploded = String(exploded);
    container.dataset.action = neutral ? 'neutral' : interest > 0.1 ? 'curious' : Math.abs(look) > 0.1 ? 'looking' : stretch > 0.1 ? 'stretch' : settle > 0.1 ? 'settle' : 'rest';
    container.dataset.pose = `${tilt.toFixed(2)},${tailDisplacement(158, time, amount, flick).toFixed(2)},${blink.toFixed(2)}`;
  }

  return {
    render,
    blink(time) { manualBlink = time; },
    call(time) { attention = time; },
    dispose() { container.replaceChildren(); },
  };
}
