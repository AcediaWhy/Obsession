// Deterministic extraction requested by the user: no generative redraw or resize.
import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { createHash } from 'node:crypto';
const require = createRequire(import.meta.url);
const sharp = require(path.join(homedir(), '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/sharp'));
const root = path.resolve(import.meta.dirname, '..');
const sourceDir = path.join(root, 'assets-src/alchemist/exact-v4');
const out = path.join(root, 'public/lab-assets/alchemist-cat/exact-v4');
await fs.mkdir(sourceDir, { recursive: true });
await fs.mkdir(out, { recursive: true });
const source = path.join(sourceDir, 'reference.png');
const { data: raw, info } = await sharp(source).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
const { width: W, height: H } = info;
if (W !== 1254 || H !== 1254) throw new Error('Unexpected reference dimensions');
// Preserve the supplied alpha; no flood fill, erosion, thresholding or matting.
const metadata = await sharp(source).metadata();
if (!metadata.hasAlpha) throw new Error('Expected the user-supplied transparent PNG');
const cut = Buffer.from(raw);
for (let i = 0; i < cut.length; i += 4) if (!cut[i + 3]) cut.fill(0, i, i + 4);

const polygons = {
  leftEar: [[326, 455], [326, 290], [356, 246], [408, 241], [508, 348], [527, 373], [498, 397], [448, 416], [390, 440]],
  rightEar: [[795, 355], [863, 285], [895, 264], [938, 270], [960, 302], [960, 504], [925, 483], [875, 453], [837, 433], [809, 390]],
  tail: [[183, 777], [365, 777], [369, 864], [316, 920], [305, 963], [351, 1010], [375, 1056], [388, 1140], [183, 1140]],
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
    if ((layers.leftEar[i + 3] && y >= 425) || (layers.rightEar[i + 3] && y >= 463)) cut.copy(roots, i, i, i + 4);
  }
}
// Small hidden hat backing, sampled from existing purple cloth. It is separate
// from exact cutouts and used only during movement, never in the neutral frame.
const backing = Buffer.alloc(cut.length);
for (let y = 300; y < 490; y++) for (let x = 329; x < 969; x++) {
  const i = (y * W + x) * 4;
  const left = layers.leftEar[i + 3] && x > 501 - (y - 300) * .60;
  const right = layers.rightEar[i + 3] && x < 899 && y > 330;
  if (left || right) {
    const sx = left ? 544 : 788, sy = Math.min(y, 390), sample = (sy * W + sx) * 4;
    cut.copy(backing, i, i, i + 4);
    backing[i] = cut[sample]; backing[i + 1] = cut[sample + 1]; backing[i + 2] = cut[sample + 2];
  }
}
// Eyelid plates sample adjacent unpainted face, not a generated replacement eye.
const skin = Buffer.alloc(cut.length);
const eyeMasks = [
  [[390, 591], [428, 550], [453, 530], [530, 530], [560, 549], [589, 582], [596, 608], [596, 673], [563, 709], [531, 723], [456, 723], [420, 698], [390, 671]],
  [[674, 598], [709, 560], [739, 538], [802, 538], [834, 558], [864, 590], [878, 613], [878, 671], [845, 710], [810, 725], [741, 725], [702, 699], [674, 672]],
];
for (const [x0, y0, w, h] of [[379, 524, 225, 208], [670, 532, 218, 202]]) {
  for (let y = y0; y < y0 + h; y++) for (let x = x0; x < x0 + w; x++) {
    if (!eyeMasks.some(polygon => inside(x + .5, y + .5, polygon))) continue;
    const i = (y * W + x) * 4, sample = (y * W + 609) * 4;
    cut.copy(skin, i, i, i + 4);
    skin[i] = cut[sample]; skin[i + 1] = cut[sample + 1]; skin[i + 2] = cut[sample + 2]; skin[i + 3] = cut[sample + 3];
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
let sourceAlphaDifferences = 0, sourceVisibleRgbDifferences = 0;
for (let i = 0; i < cut.length; i += 4) {
  if (cut[i + 3] !== raw[i + 3]) sourceAlphaDifferences++;
  if (raw[i + 3] && !cut.subarray(i, i + 3).equals(raw.subarray(i, i + 3))) sourceVisibleRgbDifferences++;
}
if (sourceAlphaDifferences || sourceVisibleRgbDifferences) throw new Error('Source artwork changed');
const report = { sourceSha256: createHash('sha256').update(await fs.readFile(source)).digest('hex'), width: W, height: H, sourceAlphaDifferences, sourceVisibleRgbDifferences, reassemblyDifferentPixels: 0, webpDecodedDifferentVisiblePixels: 0, note: 'Original user alpha preserved. No background removal or edge erosion. backing and skin are separate cloned occlusion plates.' };
await fs.writeFile(path.join(sourceDir, 'verification.json'), JSON.stringify(report, null, 2));
console.log(report);
