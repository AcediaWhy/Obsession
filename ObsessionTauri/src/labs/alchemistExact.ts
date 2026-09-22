export const exactFiles = {
  original: 'original', base: 'motion-base', tail: 'tail-tip',
  leftEar: 'leftEar', rightEar: 'rightEar', roots: 'roots', backing: 'backing', skin: 'skin',
} as const;
export type ExactImages = Record<keyof typeof exactFiles, CanvasImageSource>;
export type ExactMotionMode = 'auto' | 'still' | 'ears' | 'tail';
export interface ExactPose { blink: number; tail: number; leftEar: number; rightEar: number; action: 'idle' | 'bubble' }
export const neutralExactPose: ExactPose = { blink: 0, tail: 0, leftEar: 0, rightEar: 0, action: 'idle' };

// Angles are small rotations of the SAME extracted artwork, not replacement
// anatomical poses. Nonuniform timing provides quick attention and slow settling.
function curve(t: number, keys: readonly (readonly [number, number])[]) {
  if (t < keys[0][0] || t >= keys[keys.length - 1][0]) return 0;
  for (let i = 1; i < keys.length; i++) {
    if (t <= keys[i][0]) {
      const [a, va] = keys[i - 1], [b, vb] = keys[i];
      const s = (t - a) / (b - a), ease = s * s * (3 - 2 * s);
      return va + (vb - va) * ease;
    }
  }
  return 0;
}
const earKeys = [[0, 0], [95, -2.8], [190, .6], [390, 0]] as const;
const tailKeys = [[0, 0], [850, 4], [1750, -1.5], [2700, 0]] as const;
const blinkKeys = [[0, 0], [100, 1], [155, 1], [300, 0]] as const;
const listenKeys = [[0, 0], [220, -1.6], [520, -1.2], [1100, 0]] as const;
const snapAngle = (angle: number) => Math.round(angle * 5) / 5 || 0;

export function exactPoseAt(time: number, mood: 'rest' | 'brew' | 'ready', mode: ExactMotionMode = 'auto'): ExactPose {
  if (mode === 'still') return { ...neutralExactPose };
  const wrap = (period: number) => ((time % period) + period) % period;
  const t = wrap(23000);
  const earTime = mode === 'ears' ? wrap(6200) : t;
  const tailTime = mode === 'tail' ? wrap(6200) : t;
  const ears = mode !== 'tail';
  const tail = mode !== 'ears';
  // Low-amplitude motion continues between gestures, without moving the body.
  // Whole-number harmonics close the loop smoothly, including tail-only mode.
  const phase = tailTime / (mode === 'tail' ? 6200 : 23000) * Math.PI * 2;
  const drift = 1.8 * Math.sin(phase * (mode === 'tail' ? 1 : 4)) + .4 * Math.sin(phase);
  const tailGesture = .45 * curve(tailTime - 1400, tailKeys) - .35 * curve(tailTime - 15300, tailKeys);
  const energy = mood === 'rest' ? .85 : 1;
  return {
    blink: mode === 'auto' ? Math.round(Math.max(...[2900, 6700, 11200, 15100, 18300, 18800, 21700].map(start => curve(t - start, blinkKeys))) * 16) / 16 : 0,
    leftEar: ears ? snapAngle(curve(earTime - 650, earKeys) + curve(earTime - 4200, listenKeys) + .65 * curve(earTime - 10300, earKeys) + .6 * curve(earTime - 14200, earKeys) + curve(earTime - 20000, listenKeys)) : 0,
    rightEar: ears ? snapAngle(-.75 * curve(earTime - 900, earKeys) - .8 * curve(earTime - 3000, listenKeys) - curve(earTime - 8200, earKeys) - .7 * curve(earTime - 12800, listenKeys) - .65 * curve(earTime - 17100, earKeys)) : 0,
    tail: tail ? snapAngle((drift + tailGesture) * energy) : 0,
    action: mood === 'brew' && ((t >= 4300 && t < 6700) || (t >= 12500 && t < 14500) || (t >= 20100 && t < 21800)) ? 'bubble' : 'idle',
  };
}

export function drawExactAlchemist(canvas: HTMLCanvasElement, images: ExactImages, pose: ExactPose, pixels: number) {
  const resized = canvas.width !== pixels || canvas.height !== pixels;
  if (resized) { canvas.width = pixels; canvas.height = pixels; }
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  if (!resized) ctx.clearRect(0, 0, pixels, pixels);
  ctx.imageSmoothingEnabled = false;
  const scale = pixels / 1254;
  const layer = (image: CanvasImageSource) => ctx.drawImage(image, 0, 0, pixels, pixels);
  if (!pose.tail && !pose.leftEar && !pose.rightEar) {
    // Exact neutral rendering: no cropping, masks, independently scaled parts,
    // replacement eyes or reconstructed body. Identical to the source sprite.
    layer(images.original);
  } else {
    layer(images.backing);
    const rotate = (image: CanvasImageSource, degrees: number, x: number, y: number) => {
      ctx.save();
      const px = Math.round(x * scale), py = Math.round(y * scale);
      ctx.translate(px, py); ctx.rotate(degrees * Math.PI / 180);
      ctx.drawImage(image, -px, -py, pixels, pixels);
      ctx.restore();
    };
    rotate(images.tail, pose.tail, 264, 1002);
    rotate(images.leftEar, pose.leftEar, 419, 427);
    rotate(images.rightEar, pose.rightEar, 900, 466);
    layer(images.base);
    layer(images.roots);
  }
  if (pose.blink > 0) {
    const closure = Math.min(1, Math.max(0, pose.blink));
    // Occluding eyelids, never compressing/stretching the pupils or eyeballs.
    for (const [x, y, w, h] of [[379, 524, 225, 208], [670, 532, 218, 202]]) {
      const mid = y + h * .61;
      const top = Math.round((y + (mid - y) * closure) * scale);
      const bottom = Math.round((y + h - (y + h - mid) * closure) * scale);
      const left = Math.round(x * scale), right = Math.round((x + w) * scale);
      ctx.save(); ctx.beginPath();
      ctx.rect(left, Math.round(y * scale), right - left, top - Math.round(y * scale));
      ctx.rect(left, bottom, right - left, Math.round((y + h) * scale) - bottom);
      ctx.clip(); layer(images.skin); ctx.restore();
      const margin = Math.round(25 * scale);
      ctx.fillStyle = '#100d10';
      if (closure >= .99) {
        const thickness = Math.max(1, Math.round(11 * scale));
        ctx.fillRect(left + margin, top, right - left - margin * 2, thickness);
        ctx.fillRect(left + margin - thickness, top - thickness, thickness, thickness);
        ctx.fillRect(right - margin, top - thickness, thickness, thickness);
      } else if (closure > .25) {
        ctx.fillRect(left + margin, top, right - left - margin * 2, Math.max(1, Math.round(9 * scale)));
      }
    }
  }
}
