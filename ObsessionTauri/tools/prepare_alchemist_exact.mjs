// Deterministic extraction requested by the user: no generative redraw or resize.
import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { createHash } from 'node:crypto';
const require = createRequire(import.meta.url);
const sharp = require(path.join(homedir(), '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/sharp'));
const root = path.resolve(import.meta.dirname, '..');
const sourceDir = path.join(root, 'assets-src/alchemist/exact-v3');
const out = path.join(root, 'public/lab-assets/alchemist-cat/exact-v3');
await fs.mkdir(sourceDir, { recursive: true });
await fs.mkdir(out, { recursive: true });
const source = path.join(sourceDir, 'reference.png');
const { data: raw, info } = await sharp(source).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
const { width: W, height: H } = info;
if (W !== 1254 || H !== 1254) throw new Error('Unexpected reference dimensions');
const size = W * H, cut = Buffer.from(raw), background = new Uint8Array(size), queue = new Int32Array(size);
let head = 0, end = 0;
function enqueue(p) {
  if (background[p]) return;
  const i = p * 4, r = raw[i], g = raw[i + 1], b = raw[i + 2];
  // Only border-connected bright neutral checkerboard. Dark outlines protect
  // white eye interiors. Never replace the RGB of any retained source pixel.
  if (Math.min(r, g, b) < 105 || Math.max(r, g, b) - Math.min(r, g, b) > 18) return;
  background[p] = 1; queue[end++] = p;
}
for (let x = 0; x < W; x++) { enqueue(x); enqueue((H - 1) * W + x); }
for (let y = 0; y < H; y++) { enqueue(y * W); enqueue(y * W + W - 1); }
while (head < end) {
  const p = queue[head++], x = p % W, y = Math.floor(p / W);
  if (x > 0) enqueue(p - 1);
  if (x < W - 1) enqueue(p + 1);
  if (y > 0) enqueue(p - W);
  if (y < H - 1) enqueue(p + W);
}
for (let p = 0; p < size; p++) if (background[p]) cut.fill(0, p * 4, p * 4 + 4);

// The baked checkerboard is antialiased into the outside of the dark outline.
// Trim only two source pixels adjoining EXTERIOR background, before splitting
// parts. Never erode internal layer seams, eyes, fur or the moving-part roots.
// The artwork's outline is about 10 source pixels thick; this removes its
// contaminated fringe while keeping the dark outline and all interior RGB.
let exterior = Uint8Array.from(background), removedFringePixels = 0;
for (let pass = 0; pass < 2; pass++) {
  const expanded = Uint8Array.from(exterior);
  for (let y = 1; y < H - 1; y++) for (let x = 1; x < W - 1; x++) {
    const p = y * W + x;
    if (exterior[p]) continue;
    if ([-W - 1, -W, -W + 1, -1, 1, W - 1, W, W + 1].some(offset => exterior[p + offset])) {
      expanded[p] = 1;
      cut.fill(0, p * 4, p * 4 + 4);
      removedFringePixels++;
    }
  }
  exterior = expanded;
}
for (let i = 0; i < cut.length; i += 4) {
  if (cut[i + 3] && !cut.subarray(i, i + 4).equals(raw.subarray(i, i + 4))) throw new Error('Retained source pixel changed');
}

const polygons = {
  leftEar: [[329, 443], [329, 280], [368, 233], [419, 233], [536, 349], [539, 390], [482, 400], [425, 418], [377, 435]],
  rightEar: [[797, 350], [866, 272], [909, 247], [953, 254], [969, 291], [969, 490], [930, 490], [887, 453], [858, 433], [832, 390]],
  tail: [[181, 777], [348, 777], [349, 899], [316, 955], [322, 1000], [384, 1060], [398, 1126], [180, 1126]],
  flask: [[569, 728], [695, 728], [711, 792], [711, 827], [750, 864], [810, 858], [857, 907], [841, 984], [775, 1013], [771, 1057], [724, 1099], [573, 1104], [518, 1060], [508, 1017], [448, 1005], [409, 957], [416, 900], [465, 858], [533, 860], [574, 824], [580, 790], [564, 783]],
  eyes: [[374, 512], [902, 512], [902, 725], [374, 725]],
  hat: [[185, 50], [1124, 50], [1124, 744], [936, 744], [923, 567], [697, 505], [605, 470], [464, 444], [396, 488], [350, 559], [341, 623], [185, 623]],
};
function inside(x, y, polygon) {
  let yes = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const [xi, yi] = polygon[i], [xj, yj] = polygon[j];
    if ((yi > y) !== (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi) yes = !yes;
  }
  return yes;
}
const layers = Object.fromEntries([...Object.keys(polygons), 'body'].map(name => [name, Buffer.alloc(cut.length)]));
for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
  const i = (y * W + x) * 4;
  if (!cut[i + 3]) continue;
  const name = Object.entries(polygons).find(([, polygon]) => inside(x + .5, y + .5, polygon))?.[0] ?? 'body';
  cut.copy(layers[name], i, i, i + 4);
}
const motionBase = Buffer.from(cut), tailTip = Buffer.alloc(cut.length), roots = Buffer.alloc(cut.length);
for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
  const i = (y * W + x) * 4;
  if (layers.tail[i + 3] && y < 1016) {
    cut.copy(tailTip, i, i, i + 4); motionBase.fill(0, i, i + 4);
    if (y >= 988) cut.copy(roots, i, i, i + 4);
  }
  if (layers.leftEar[i + 3] || layers.rightEar[i + 3]) {
    motionBase.fill(0, i, i + 4);
    if ((layers.leftEar[i + 3] && y >= 405) || (layers.rightEar[i + 3] && y >= 445)) cut.copy(roots, i, i, i + 4);
  }
}
// Small hidden hat backing, sampled from existing purple cloth. It is separate
// from exact cutouts and used only during movement, never in the neutral frame.
const backing = Buffer.alloc(cut.length);
for (let y = 300; y < 490; y++) for (let x = 329; x < 969; x++) {
  const i = (y * W + x) * 4;
  const left = layers.leftEar[i + 3] && x > 490 - (y - 300) * .68;
  const right = layers.rightEar[i + 3] && x < 916 && y > 312;
  if (left || right) {
    const sx = left ? 544 : 788, sy = Math.min(y, 390), sample = (sy * W + sx) * 4;
    cut.copy(backing, i, i, i + 4);
    backing[i] = cut[sample]; backing[i + 1] = cut[sample + 1]; backing[i + 2] = cut[sample + 2];
  }
}
// Eyelid plates sample adjacent unpainted face, not a generated replacement eye.
const skin = Buffer.alloc(cut.length);
const eyeMasks = [
  [[392, 590], [426, 548], [459, 524], [529, 524], [560, 549], [585, 578], [596, 606], [596, 659], [569, 688], [532, 714], [464, 714], [426, 690], [392, 663]],
  [[680, 597], [704, 565], [738, 532], [808, 532], [840, 555], [867, 582], [887, 610], [887, 658], [862, 689], [817, 714], [744, 714], [711, 690], [680, 661]],
];
for (const [x0, y0, w, h] of [[379, 518, 225, 210], [679, 524, 220, 204]]) {
  for (let y = y0; y < y0 + h; y++) for (let x = x0; x < x0 + w; x++) {
    if (!eyeMasks.some(polygon => inside(x + .5, y + .5, polygon))) continue;
    const i = (y * W + x) * 4, sample = (y * W + 609) * 4;
    cut.copy(skin, i, i, i + 4);
    skin[i] = cut[sample]; skin[i + 1] = cut[sample + 1]; skin[i + 2] = cut[sample + 2]; skin[i + 3] = 255;
  }
}
async function save(name, data) {
  await sharp(data, { raw: { width: W, height: H, channels: 4 } }).png().toFile(path.join(sourceDir, `${name}.png`));
  const webp = await sharp(data, { raw: { width: W, height: H, channels: 4 } }).webp({ lossless: true, effort: 6 }).toBuffer();
  await fs.writeFile(path.join(out, `${name}.webp`), webp);
  const decoded = await sharp(webp).ensureAlpha().raw().toBuffer();
  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] !== decoded[i + 3] || (data[i + 3] && !data.subarray(i, i + 3).equals(decoded.subarray(i, i + 3)))) throw new Error(`WebP mismatch: ${name}`);
  }
}
const recomposed = Buffer.alloc(cut.length);
for (const data of Object.values(layers)) for (let i = 0; i < data.length; i += 4) if (data[i + 3]) data.copy(recomposed, i, i, i + 4);
if (!recomposed.equals(cut)) throw new Error('Cutout round-trip is not exact');
for (const [name, data] of Object.entries({ original: cut, ...layers, 'motion-base': motionBase, 'tail-tip': tailTip, roots, backing, skin })) await save(name, data);
const report = { sourceSha256: createHash('sha256').update(await fs.readFile(source)).digest('hex'), width: W, height: H, removedBackgroundPixels: end, removedFringePixels, exteriorTrimSourcePixels: 2, retainedPixels: size - end - removedFringePixels, retainedRgbChanges: 0, reassemblyDifferentPixels: 0, webpDecodedDifferentVisiblePixels: 0, note: 'Background alpha extraction plus two-source-pixel exterior fringe trim; neutral cutouts exact to cleaned reference. backing and skin are separately identified clone-painted occlusion plates, not original art.' };
await fs.writeFile(path.join(sourceDir, 'verification.json'), JSON.stringify(report, null, 2));
console.log(report);
