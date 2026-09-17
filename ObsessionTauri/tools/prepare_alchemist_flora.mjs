// Source-preserving cutouts approved for the flower animation. Imagegen supplies
// only hidden background pixels; it never supplies the flower or the books.
import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
const require = createRequire(import.meta.url);
const sharp = require(path.join(homedir(), '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/sharp'));
const root = path.resolve(import.meta.dirname, '..');
const sourceDir = path.join(root, 'assets-src/alchemist/scene-v2-flora');
const outputDir = path.join(root, 'public/lab-assets/alchemist-cat/scene-v2-flora');
await fs.mkdir(sourceDir, { recursive: true });
await fs.mkdir(outputDir, { recursive: true });
const source = path.join(root, 'assets-src/alchemist/scene-v1/workshop.png');
const plate = path.join(sourceDir, 'backplate-generated.png');
const crop = { left: 1380, top: 480, width: 156, height: 210 };
await sharp(source).extract(crop).resize(624, 840, { kernel: 'nearest' }).png().toFile(path.join(sourceDir, 'source-detail.png'));
await sharp(plate).extract(crop).resize(624, 840, { kernel: 'nearest' }).png().toFile(path.join(sourceDir, 'backplate-detail.png'));
const sourceRaw = await sharp(source).extract(crop).ensureAlpha().raw().toBuffer();
const plateRaw = await sharp(plate).extract(crop).ensureAlpha().raw().toBuffer();
// Coordinates in the source crop, traced around the original petals and stalks.
// Small dark edge pixels are retained rather than eroded or recoloured.
const polygons = {
  tall: [[68,19],[79,19],[79,28],[88,28],[88,39],[95,39],[95,50],[100,50],[100,70],[106,71],[106,80],[100,81],[100,86],[95,86],[95,96],[94,104],[99,105],[100,116],[104,117],[107,147],[102,154],[97,145],[94,122],[91,120],[90,105],[90,96],[77,96],[77,86],[68,85],[68,73],[77,72],[77,63],[68,63],[68,51],[61,51],[61,39],[68,39]],
  left: [[31,83],[44,83],[45,90],[52,89],[53,96],[59,97],[61,105],[60,112],[67,111],[70,116],[76,116],[76,128],[69,128],[69,138],[74,138],[75,148],[79,151],[80,161],[76,161],[73,151],[69,149],[68,138],[52,138],[51,128],[40,128],[40,120],[44,119],[44,114],[36,114],[36,96],[31,96]],
  right: [[136,43],[145,43],[146,64],[151,64],[151,80],[146,80],[146,86],[148,86],[148,95],[138,96],[138,106],[126,107],[126,117],[124,117],[123,131],[118,135],[116,148],[112,153],[108,150],[110,135],[113,130],[114,118],[118,116],[119,106],[122,105],[122,96],[114,95],[114,84],[122,84],[123,78],[117,78],[117,73],[122,72],[122,61],[129,61],[129,50],[136,50]],
};
function inside(x, y, polygon) {
  let result = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const [xi, yi] = polygon[i], [xj, yj] = polygon[j];
    if ((yi > y) !== (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi) result = !result;
  }
  return result;
}
const layers = Object.fromEntries(Object.keys(polygons).map(name => [name, Buffer.alloc(sourceRaw.length)]));
const backing = Buffer.alloc(sourceRaw.length);
let extracted = 0;
for (let y = 0; y < crop.height; y++) for (let x = 0; x < crop.width; x++) {
  const name = Object.keys(polygons).find(key => inside(x + .5, y + .5, polygons[key]));
  if (!name) continue;
  const i = (y * crop.width + x) * 4;
  sourceRaw.copy(layers[name], i, i, i + 4);
  plateRaw.copy(backing, i, i, i + 4);
  extracted++;
}
const assembled = Buffer.from(sourceRaw);
for (let i = 0; i < assembled.length; i += 4) if (backing[i + 3]) backing.copy(assembled, i, i, i + 4);
for (const layer of Object.values(layers)) for (let i = 0; i < layer.length; i += 4) if (layer[i + 3]) layer.copy(assembled, i, i, i + 4);
if (!assembled.equals(sourceRaw)) throw new Error('Neutral pose does not match the original');
const rawOptions = { raw: { width: crop.width, height: crop.height, channels: 4 } };
const sizes = {};
for (const [name, data] of Object.entries({ backing, ...layers })) {
  const file = path.join(outputDir, `${name}.webp`);
  await sharp(data, rawOptions).webp({ lossless: true, effort: 6 }).toFile(file);
  const decoded = await sharp(file).ensureAlpha().raw().toBuffer();
  for (let i = 0; i < data.length; i += 4) {
    if (decoded[i + 3] !== data[i + 3] || (data[i + 3] && !decoded.subarray(i, i + 3).equals(data.subarray(i, i + 3)))) throw new Error(`Lossy output: ${name}`);
  }
  sizes[name] = (await fs.stat(file)).size;
}
const report = { crop, extractedPixels: extracted, neutralDifferentPixels: 0, losslessVerified: true, sizes };
await fs.writeFile(path.join(sourceDir, 'verification.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
