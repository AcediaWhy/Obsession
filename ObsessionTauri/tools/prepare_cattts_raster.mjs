// Asset preparation only. The browser receives raster images; no SVG is loaded there.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';

const require = createRequire(import.meta.url);
let sharp;
try { sharp = require('sharp'); }
catch { sharp = require(path.join(homedir(), '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/sharp')); }
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const sourceDir = path.join(root, 'assets-src/cattts-raster-v1');
const out = path.join(root, 'lab-assets/cattts-raster-v1');
await fs.mkdir(out, { recursive: true });
for (const name of ['original', 'animal-clean-plate', 'folded-pose', 'leaf-clean-plate']) {
  await sharp(path.join(sourceDir, `${name}.png`)).resize(1024, 1024).webp({ lossless: true }).toFile(path.join(out, `${name}.webp`));
}

// Saved raster selection, initially derived from the earlier leaf tracing.
// Rebuilding this study no longer requires the SVG or the user's Downloads.
const leafStencil = await sharp(path.join(sourceDir, 'leaf-stencil.webp')).png().toBuffer();
const stencilBounds = await sharp(leafStencil).trim().toBuffer({ resolveWithObject: true });
const meta = { size: 1024, leaf: { x: -stencilBounds.info.trimOffsetLeft, y: -stencilBounds.info.trimOffsetTop, w: stencilBounds.info.width, h: stencilBounds.info.height } };
const leafPixels = await sharp(path.join(out, 'original.webp')).ensureAlpha().raw().toBuffer();
const maskPixels = await sharp(leafStencil).ensureAlpha().raw().toBuffer();
for (let i = 3; i < leafPixels.length; i += 4) leafPixels[i] = maskPixels[i];
await sharp(leafPixels, { raw: { width: 1024, height: 1024, channels: 4 } })
  .extract({ left: meta.leaf.x, top: meta.leaf.y, width: meta.leaf.w, height: meta.leaf.h })
  .webp({ lossless: true }).toFile(path.join(out, 'leaf.webp'));
await sharp(leafStencil).webp({ lossless: true }).toFile(path.join(out, 'leaf-mask.webp'));
await fs.writeFile(path.join(out, 'layout.json'), JSON.stringify(meta, null, 2));
await prepareRasterFrames();
console.log(meta);
for (const name of await fs.readdir(out)) {
  console.log(name, (await fs.stat(path.join(out, name))).size);
}

async function prepareRasterFrames() {
  const pixels = {};
  for (const name of ['original', 'folded-pose', 'animal-clean-plate', 'leaf-clean-plate']) {
    pixels[name] = await sharp(path.join(out, `${name}.webp`)).ensureAlpha().raw().toBuffer();
  }
  const { leaf } = meta;
  meta.leafBacking = { x: Math.max(0, leaf.x - 3), y: Math.max(0, leaf.y - 3), w: Math.min(1024, leaf.x + leaf.w + 3) - Math.max(0, leaf.x - 3), h: leaf.h + 6 };
  const cleanRect = meta.leafBacking;
  const leafBacking = Buffer.from(pixels.original);
  // Match the visible boundary to the original neighbouring paint. The generated
  // clean plate supplies deeper hidden areas; it must not introduce a dark ring
  // where its newly painted leaves meet the old foreground.
  const safeExterior = new Uint8Array(1024 * 1024), offsets = [];
  for (let dy = -16; dy <= 16; dy++) for (let dx = -16; dx <= 16; dx++) if (dx * dx + dy * dy <= 256) offsets.push([dx, dy, dx * dx + dy * dy]);
  offsets.sort((a, b) => a[2] - b[2]);
  for (let y = Math.max(2, cleanRect.y - 18); y < Math.min(1022, cleanRect.y + cleanRect.h + 18); y++) for (let x = Math.max(2, cleanRect.x - 18); x < 1022; x++) {
    let clear = true;
    for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) if (maskPixels[((y + dy) * 1024 + x + dx) * 4 + 3] > 5) clear = false;
    safeExterior[y * 1024 + x] = clear ? 1 : 0;
  }
  for (let y = cleanRect.y; y < cleanRect.y + cleanRect.h; y++) for (let x = cleanRect.x; x < cleanRect.x + cleanRect.w; x++) {
    const i = y * 1024 + x;
    let a = 0;
    for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) {
      if (x + dx >= 0 && x + dx < 1024 && y + dy >= 0 && y + dy < 1024) a = Math.max(a, maskPixels[((y + dy) * 1024 + x + dx) * 4 + 3] / 255);
    }
    if (!a) continue;
    let nearest = -1, separation = 16;
    for (const [dx, dy, d2] of offsets) {
      if (x + dx < 0 || x + dx > 1023 || y + dy < 0 || y + dy > 1023) continue;
      const candidate = (y + dy) * 1024 + x + dx;
      if (safeExterior[candidate]) { nearest = candidate; separation = Math.sqrt(d2); break; }
    }
    const blend = Math.max(0, Math.min(1, (separation - 7) / 9));
    const generatedWeight = blend * blend * (3 - 2 * blend);
    for (let c = 0; c < 3; c++) {
      const hidden = nearest < 0 ? pixels['leaf-clean-plate'][i * 4 + c] : pixels.original[nearest * 4 + c] * (1 - generatedWeight) + pixels['leaf-clean-plate'][i * 4 + c] * generatedWeight;
      leafBacking[i * 4 + c] = Math.round(leafBacking[i * 4 + c] * (1 - a) + hidden * a);
    }
  }
  await sharp(leafBacking, { raw: { width: 1024, height: 1024, channels: 4 } })
    .extract({ left: cleanRect.x, top: cleanRect.y, width: cleanRect.w, height: cleanRect.h })
    .webp({ lossless: true }).toFile(path.join(out, 'leaf-backing.webp'));

  // Positive inside, negative outside. Chamfer distances are enough for these
  // small antialiased silhouettes, and preserve one contour instead of ghosting
  // two overlapping poses. All frames are baked offline.
  const distance = (mask, w, h, inside) => {
    const d = new Float32Array(w * h);
    for (let i = 0; i < d.length; i++) d[i] = Boolean(mask[i]) === inside ? 0 : 10000;
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = y * w + x;
      if (x) d[i] = Math.min(d[i], d[i - 1] + 1);
      if (y) d[i] = Math.min(d[i], d[i - w] + 1);
      if (x && y) d[i] = Math.min(d[i], d[i - w - 1] + Math.SQRT2);
      if (y && x < w - 1) d[i] = Math.min(d[i], d[i - w + 1] + Math.SQRT2);
    }
    for (let y = h - 1; y >= 0; y--) for (let x = w - 1; x >= 0; x--) {
      const i = y * w + x;
      if (x < w - 1) d[i] = Math.min(d[i], d[i + 1] + 1);
      if (y < h - 1) d[i] = Math.min(d[i], d[i + w] + 1);
      if (x < w - 1 && y < h - 1) d[i] = Math.min(d[i], d[i + w + 1] + Math.SQRT2);
      if (x && y < h - 1) d[i] = Math.min(d[i], d[i + w - 1] + Math.SQRT2);
    }
    return d;
  };
  const clamp = v => Math.max(0, Math.min(1, v));
  const smooth = v => { const t = clamp(v); return t * t * (3 - 2 * t); };
  meta.parts = [
    { name: 'ear', x: 392, y: 272, w: 164, h: 118, anchorY: 345, anchorEnd: 373 },
    { name: 'paw', x: 476, y: 582, w: 132, h: 102, anchorY: 599, anchorEnd: 584 },
  ];
  const inkIndex = (450 * 1024 + 450) * 4;
  const ink = [...pixels.original.subarray(inkIndex, inkIndex + 3)];
  for (const part of meta.parts) {
    const { w, h, x: left, y: top } = part;
    const maps = [], crops = [];
    for (const name of ['original', 'folded-pose']) {
      const crop = Buffer.alloc(w * h * 4), mask = new Uint8Array(w * h);
      for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
        const i = y * w + x, p = ((y + top) * 1024 + x + left) * 4;
        pixels[name].copy(crop, i * 4, p, p + 4);
        mask[i] = crop[i * 4] < 112 && crop[i * 4 + 1] < 78 && crop[i * 4 + 2] < 72 ? 1 : 0;
      }
      const exterior = distance(mask, w, h, true), interior = distance(mask, w, h, false);
      maps.push(Float32Array.from(exterior, (v, i) => interior[i] - v));
      crops.push(crop);
    }
    const backing = Buffer.from(crops[0]);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = y * w + x, p = ((y + top) * 1024 + x + left) * 4;
      const cover = clamp(maps[0][i] + 1.8);
      for (let c = 0; c < 3; c++) backing[i * 4 + c] = Math.round(backing[i * 4 + c] * (1 - cover) + pixels['animal-clean-plate'][p + c] * cover);
    }
    part.frames = 49;
    part.columns = 7;
    const atlasW = w * part.columns, atlas = Buffer.alloc(atlasW * h * 7 * 4);
    for (let frame = 0; frame < part.frames; frame++) {
      const t = frame / (part.frames - 1), rgba = Buffer.from(backing);
      for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
        const i = y * w + x;
        const weight = smooth((top + y - part.anchorEnd) / (part.anchorY - part.anchorEnd));
        const d = maps[0][i] * (1 - t * weight) + maps[1][i] * t * weight;
        const alpha = clamp(.5 + d / 1.5);
        for (let c = 0; c < 3; c++) rgba[i * 4 + c] = Math.round(backing[i * 4 + c] * (1 - alpha) + ink[c] * alpha);
        if (frame === 0 || weight === 0) crops[0].copy(rgba, i * 4, i * 4, i * 4 + 4);
      }
      const ax = frame % 7 * w, ay = Math.floor(frame / 7) * h;
      for (let y = 0; y < h; y++) rgba.copy(atlas, ((ay + y) * atlasW + ax) * 4, y * w * 4, (y + 1) * w * 4);
    }
    await sharp(atlas, { raw: { width: atlasW, height: h * 7, channels: 4 } }).webp({ lossless: true }).toFile(path.join(out, `${part.name}-poses.webp`));
  }
  await fs.writeFile(path.join(out, 'layout.json'), JSON.stringify(meta, null, 2));
  console.log('49 prepared poses per part; original ink:', ink);
}
