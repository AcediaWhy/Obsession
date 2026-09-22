import React from 'react';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { AlchemistSprite } from '../../src/design/components/AlchemistSprite';
import { loadAlchemistBitmaps, closeAlchemistBitmaps } from '../../src/labs/alchemistBitmaps';
import { drawExactAlchemist, neutralExactPose } from '../../src/labs/alchemistExact';

// Development-only diagnostic: does not import the app stores or call Tauri.
const report = document.querySelector('#report')!;
const root = createRoot(document.querySelector('#root')!);
const nativeDecode = window.createImageBitmap.bind(window);
let live = 0, bytes = 0, peakBytes = 0, created = 0, closed = 0;
function track(bitmap: ImageBitmap) {
  const size = bitmap.width * bitmap.height * 4;
  live++; created++; bytes += size; peakBytes = Math.max(peakBytes, bytes);
  const close = bitmap.close.bind(bitmap);
  let released = false;
  bitmap.close = () => { if (!released) { released = true; live--; closed++; bytes -= size; } close(); };
  return bitmap;
}
window.createImageBitmap = (async (...args: Parameters<typeof createImageBitmap>) => track(await (nativeDecode as Function)(...args))) as typeof createImageBitmap;
const nativeTransfer = OffscreenCanvas.prototype.transferToImageBitmap;
OffscreenCanvas.prototype.transferToImageBitmap = function () { return track(nativeTransfer.call(this)); };
const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
const observations: unknown[] = [];
function snapshot(label: string) { observations.push({ label, live, bytes, peakBytes, created, closed }); }
async function run() {
  const base = '/lab-assets/alchemist-cat/exact-v4/';
  const original = new Image(); original.src = `${base}original.webp`; await original.decode();
  const comparisons = [];
  for (const size of [104, 240, 480]) {
    const oldCanvas = document.createElement('canvas'), nextCanvas = document.createElement('canvas');
    oldCanvas.width = oldCanvas.height = size;
    const ctx = oldCanvas.getContext('2d')!; ctx.imageSmoothingEnabled = false; ctx.drawImage(original, 0, 0, size, size);
    const images = await loadAlchemistBitmaps(base, size, new AbortController().signal);
    drawExactAlchemist(nextCanvas, images, neutralExactPose, size);
    const a = ctx.getImageData(0, 0, size, size).data, b = nextCanvas.getContext('2d')!.getImageData(0, 0, size, size).data;
    let different = 0; for (let i = 0; i < a.length; i += 4) if (a[i] !== b[i] || a[i+1] !== b[i+1] || a[i+2] !== b[i+2] || a[i+3] !== b[i+3]) different++;
    comparisons.push({ size, differentPixels: different, totalPixels: size * size });
    closeAlchemistBitmaps(images); oldCanvas.width = nextCanvas.width = 0;
  }
  original.src = '';
  for (let cycle = 0; cycle < 5; cycle++) {
    flushSync(() => root.render(<AlchemistSprite size={240} mood="rest" label="Алхимик" paused={false} eyeMode="auto" replayKey={0} motionMode="auto" reference={false} previewTime={null} />));
    const deadline = performance.now() + 15000;
    while (!document.querySelector('[data-loaded="true"]')) { if (performance.now() > deadline) throw new Error('load timeout'); await wait(30); }
    snapshot(`mounted-${cycle}`);
    await wait(150);
    flushSync(() => root.render(null));
    await wait(50);
    snapshot(`unmounted-${cycle}`);
    if (live !== 0) throw new Error('Unreleased bitmap');
  }
  report.textContent = JSON.stringify({ passed: true, dpr: devicePixelRatio, comparisons, observations }, null, 2);
  // Leave a live preview below the completed measurements for visual inspection.
  root.render(<AlchemistSprite size={240} mood="brew" label="Алхимик" paused={false} eyeMode="auto" replayKey={0} motionMode="auto" reference={false} previewTime={null} />);
}
run().catch(error => { report.textContent = JSON.stringify({ passed: false, error: String(error), observations }); });
