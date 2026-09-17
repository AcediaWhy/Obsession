// Procedural study based on the composition of the supplied jeeklaart reference.
// The source illustration is displayed separately, never sampled by this renderer.
export function createGoldenMeadow(canvas, configuration = {}) {
  const buildStarted = performance.now();
  const W = 735, H = 505;
  const bounds = canvas.getBoundingClientRect?.();
  const displayDensity = bounds?.width > 0 && bounds?.height > 0
    ? Math.max(bounds.width / W, bounds.height / H) * (globalThis.devicePixelRatio || 1)
    : 2;
  const SCALE = configuration.scale ?? Math.min(2, Math.max(1, Math.ceil(displayDensity * 4) / 4));
  const diagnostics = canvas.dataset ?? {};
  const makeCanvas = () => typeof document === 'undefined' ? new OffscreenCanvas(1, 1) : document.createElement('canvas');
  const pixelWidth = Math.ceil(W * SCALE), pixelHeight = Math.ceil(H * SCALE);
  canvas.width = pixelWidth; canvas.height = pixelHeight;
  const ctx = canvas.getContext('2d', { alpha: false });
  let seed = 193771;
  const random = () => { seed |= 0; seed = seed + 0x6D2B79F5 | 0; let n = Math.imul(seed ^ seed >>> 15, 1 | seed); n = n + Math.imul(n ^ n >>> 7, 61 | n) ^ n; return ((n ^ n >>> 14) >>> 0) / 4294967296; };
  const range = (a, b) => a + (b - a) * random();
  const layer = (w = W, h = H, density = SCALE) => { const c = makeCanvas(); c.width = Math.ceil(w * density); c.height = Math.ceil(h * density); const g = c.getContext('2d'); g.scale(density, density); return { canvas: c, ctx: g, w, h }; };
  const drawLayer = (g, l, x = 0, y = 0, w = l.w, h = l.h) => g.drawImage(l.canvas, x, y, w, h);
  const ink = '#151b18';
  const dryPatch = (g, x, y, rx, ry, color, density = 1) => {
    // Compose stipple in a pixel buffer; millions of Canvas fill commands stall
    // the browser's first paint even though these layers are only built once.
    const sw = Math.ceil(rx * 2.5 * SCALE), sh = Math.ceil(ry * 2.5 * SCALE);
    const stamp = makeCanvas(); stamp.width = sw; stamp.height = sh;
    const stampCtx = stamp.getContext('2d'), pixels = stampCtx.createImageData(sw, sh);
    const data = pixels.data, rgb = parseInt(color.slice(1), 16);
    for (let i = 0; i < rx * ry * 3.5 * density; i++) {
      const angle = range(0, Math.PI * 2), radius = Math.sqrt(random());
      const edge = 1 + .1 * Math.sin(angle * 7) + .07 * Math.cos(angle * 13) + range(-.05, .05);
      const px = Math.floor(sw / 2 + Math.cos(angle) * radius * rx * edge * SCALE);
      const py = Math.floor(sh / 2 + Math.sin(angle) * radius * ry * edge * SCALE);
      const alpha = range(.15, .62) * (1 - radius * .52);
      const width = Math.max(1, Math.round(range(.45, 1.65) * SCALE)), height = Math.max(1, Math.round(range(.4, 1.9) * SCALE));
      for (let yy = Math.max(0, py); yy < Math.min(sh, py + height); yy++) {
        for (let xx = Math.max(0, px); xx < Math.min(sw, px + width); xx++) {
          const p = (yy * sw + xx) * 4 + 3;
          data[p] += (255 - data[p]) * alpha;
        }
      }
    }
    for (let i = 0; i < data.length; i += 4) { data[i] = rgb >> 16; data[i + 1] = rgb >> 8 & 255; data[i + 2] = rgb & 255; }
    stampCtx.putImageData(pixels, 0, 0);
    g.drawImage(stamp, x - sw / SCALE / 2, y - sh / SCALE / 2, sw / SCALE, sh / SCALE);
    stamp.width = stamp.height = 1;
  };
  const traceBlade = (path, x, y, length, lean, width) => {
    path.moveTo(x - width / 2, y);
    path.quadraticCurveTo(x + lean * .28, y - length * .62, x + lean, y - length);
    path.quadraticCurveTo(x + lean * .38 + width, y - length * .48, x + width / 2, y);
  };
  const blade = (g, x, y, length, lean, width, color) => {
    g.fillStyle = color; g.beginPath(); traceBlade(g, x, y, length, lean, width); g.fill();
  };
  const grass = (g, count, x0, x1, y0, y1, minH, maxH, palette, opacity = 1, thickness = 1) => {
    g.save();
    for (let i = 0; i < count; i++) {
      const x = range(x0, x1), y = range(y0, y1), h = range(minH, maxH);
      g.globalAlpha = opacity * range(.15, .65);
      blade(g, x, y, h, range(-h * .45, h * .45), range(.25, .95) * thickness, palette[Math.floor(random() * palette.length)]);
    }
    g.restore();
  };
  const paper = layer();
  const paperData = paper.ctx.createImageData(pixelWidth, pixelHeight);
  const coarseW = 47, coarse = new Float32Array(coarseW * 34);
  for (let i = 0; i < coarse.length; i++) coarse[i] = random();
  for (let y = 0; y < pixelHeight; y++) for (let x = 0; x < pixelWidth; x++) {
    const gx = x / 32, gy = y / 32, ix = Math.floor(gx), iy = Math.floor(gy), tx = gx - ix, ty = gy - iy;
    const field = (coarse[iy * coarseW + ix] * (1 - tx) + coarse[iy * coarseW + ix + 1] * tx) * (1 - ty) + (coarse[(iy + 1) * coarseW + ix] * (1 - tx) + coarse[(iy + 1) * coarseW + ix + 1] * tx) * ty;
    const value = Math.round(120 + (random() - .5) * 125 + (field - .5) * 58);
    const p = (y * pixelWidth + x) * 4;
    paperData.data[p] = value; paperData.data[p + 1] = value; paperData.data[p + 2] = value; paperData.data[p + 3] = 255;
  }
  paper.ctx.putImageData(paperData, 0, 0);

  const field = layer();
  field.ctx.fillStyle = '#f4ac2f'; field.ctx.fillRect(0, 0, W, H);
  for (const [x, y, rx, ry] of [[95, 32, 170, 55], [440, 30, 200, 58], [720, 60, 105, 80], [55, 180, 120, 52], [308, 159, 190, 48], [560, 155, 180, 54], [67, 310, 100, 80], [350, 286, 115, 78], [673, 319, 140, 70]]) {
    dryPatch(field.ctx, x, y, rx, ry, '#ffd16a', 1.35);
  }
  for (let i = 0; i < 90; i++) {
    const pale = random() < .5, x = range(-20, W + 20), y = range(0, H);
    const rx = range(16, 65), ry = range(9, 32);
    dryPatch(field.ctx, x, y, rx, ry, pale ? '#ffd777' : '#de9522', .85);
    grass(field.ctx, 90, x - rx, x + rx, y, y + ry, 3, 15, pale ? ['#ffdc84', '#ffd16a'] : ['#e69b25', '#cc821b'], .8);
  }
  for (const [x, y, rx, ry] of [[51, 106, 74, 28], [346, 91, 95, 29], [672, 128, 77, 34], [139, 227, 78, 28], [583, 248, 88, 33], [42, 345, 48, 25]]) {
    dryPatch(field.ctx, x, y, rx, ry, '#d98b1e', .9);
    grass(field.ctx, 220, x - rx, x + rx, y + ry * .35, y + ry, 5, 22, ['#eaaa33', '#f8c45a', '#ffd16e'], .85);
  }
  grass(field.ctx, 23000, -15, W + 15, 0, H + 15, 2, 13, ['#efbd5c', '#ffdd85', '#e29b2c', '#d18b22'], .52);

  const pumpkin = (white = false, variant = 0) => {
    const s = layer(160, 150), g = s.ctx;
    g.translate(80, 83);
    const shapes = [
      'M0 -48 C-15 -57 -30 -51 -35 -43 C-46 -47 -55 -30 -57 -20 C-72 10 -57 44 -39 49 C-28 57 -7 59 3 55 C23 59 39 51 44 46 C65 35 69 7 58 -19 C53 -38 40 -51 26 -46 C17 -54 7 -52 0 -48Z',
      'M-2 -52 C-18 -61 -34 -45 -38 -35 C-47 -34 -54 -15 -53 3 C-56 29 -42 51 -25 53 C-12 58 6 59 18 53 C34 55 48 41 51 20 C57 3 51 -21 41 -35 C33 -48 17 -59 7 -52 Q1 -50 -2 -52Z',
      'M0 -35 C-19 -44 -32 -39 -37 -32 C-54 -35 -64 -17 -65 -8 C-78 15 -58 39 -43 44 C-28 50 -9 52 4 48 C21 52 35 45 43 42 C62 38 74 18 65 -1 C63 -19 46 -37 33 -34 C20 -43 8 -39 0 -35Z',
    ];
    const crown = [-47, -51, -34][variant];
    const shape = new Path2D(shapes[variant]);
    const fill = g.createLinearGradient(-65, -20, 65, 35);
    fill.addColorStop(0, white ? '#f6f3e6' : ['#f59708', '#f18b12', '#eb8a11'][variant]);
    fill.addColorStop(.38, white ? '#fffdf0' : ['#ffa90a', '#f59a0e', '#f99b12'][variant]);
    fill.addColorStop(1, white ? '#eae6d2' : ['#ed880b', '#e98510', '#e98113'][variant]);
    g.fillStyle = fill; g.fill(shape);
    g.save(); g.clip(shape);
    const ribs = variant === 2 ? [-.74, -.32, .23, .67] : [-.68, -.27, .31, .7];
    for (const amount of ribs) {
      const top = crown - 3 + Math.abs(amount) * 5;
      const rib = new Path2D(); rib.moveTo(amount * 40, top);
      rib.bezierCurveTo(amount * 73 - 2, top + 17, amount * 82 + 3, 28, amount * 38, 53);
      // Broad, low-contrast pigment beside each crease gives the lobes volume
      // without the regular bright piping of the earlier pumpkin sprites.
      g.strokeStyle = white ? 'rgba(170,165,138,.055)' : 'rgba(192,108,14,.065)';
      g.lineWidth = 5.5; g.stroke(rib);
      g.strokeStyle = white ? 'rgba(157,152,130,.25)' : 'rgba(186,108,17,.29)';
      g.lineWidth = 1.15; g.stroke(rib);
    }
    for (let i = 0; i < 8500; i++) {
      const x = range(-70, 70), y = range(-58, 60);
      const pigment = .5 + .25 * Math.sin(x * .12 + Math.sin(y * .09)) + .25 * Math.cos(y * .17 - x * .05);
      g.fillStyle = i % 3 ? (white ? `rgba(157,150,121,${.035 + pigment * .07})` : `rgba(197,108,10,${.045 + pigment * .11})`) : (white ? 'rgba(255,255,248,.19)' : `rgba(255,193,63,${.11 + pigment * .16})`);
      g.fillRect(x, y, range(.3, 1.1), range(.4, 1.5));
    }
    g.restore();
    g.fillStyle = white ? '#8b7547' : '#806132';
    const bend = variant === 1 ? -5 : variant === 2 ? 7 : -4;
    g.beginPath(); g.moveTo(-5, crown + 3); g.quadraticCurveTo(0, crown - 1, bend - 3, crown - 9); g.lineTo(bend + 2, crown - 12); g.quadraticCurveTo(bend + 5, crown - 3, 3, crown + 3); g.closePath(); g.fill();
    g.strokeStyle = '#b39350'; g.lineWidth = .9; g.beginPath(); g.moveTo(0, crown); g.quadraticCurveTo(bend + 2, crown - 5, bend, crown - 9); g.stroke();
    return s;
  };
  const oranges = [pumpkin(), pumpkin(false, 1), pumpkin(false, 2)], ivory = pumpkin(true);
  const placePumpkin = (g, x, y, w, h, variant = 0, rotation = 0, white = false, opacity = 1) => {
    g.save(); g.globalAlpha = opacity; g.translate(x + w / 2, y + h * .6); g.rotate(rotation);
    drawLayer(g, white ? ivory : oranges[variant], -w / 2, -h * .6, w, h); g.restore();
  };
  const fieldTuft = (g, x, y, width, height, density, light = false, moving = null, windSites = null) => {
    g.save();
    const palette = light ? ['#f7cd6b', '#ffd477', '#edb650'] : ['#d69a31', '#e9b34b', '#f6c764'];
    const windSite = moving ? windSites.push([x, y]) - 1 : -1;
    for (let i = 0; i < density; i++) {
      const offset = (random() + random() - 1) * width * .5;
      const h = height * range(.35, 1) * (1 - Math.abs(offset) / width * .5);
      const alpha = range(.35, .83);
      const spec = [x + offset, y + range(-2, 4), h, offset * .35 + range(-h * .45, h * .45), range(.45, 1.1), palette[i % 3]];
      if (moving && i % 5 === 0) moving.push({ spec, alpha, windSite });
      else { g.globalAlpha = alpha; blade(g, ...spec); }
    }
    g.restore();
  };
  const fallenLeaf = (g, x, y, size, rotation, type, color) => {
    const shapes = [
      'M0 12 L-3 7 L-9 10 L-8 4 L-14 0 L-7 -2 L-9 -9 L-3 -6 L0 -15 L4 -6 L10 -10 L8 -2 L14 0 L8 5 L9 10 L3 8Z',
      'M0 13 C-2 7 -8 9 -7 4 C-15 2 -9 -3 -6 -3 C-12 -9 -4 -11 -2 -9 Q0 -17 3 -11 C10 -13 11 -7 7 -4 C15 -4 14 3 8 4 Q11 11 3 9Z',
      'M0 13 C-11 7 -13 -4 -3 -14 Q0 -18 2 -14 C13 -7 12 6 0 13Z',
      'M-21 3 C-13 -5 1 -9 18 -2 L22 -1 Q10 1 8 5 C-3 9 -14 8 -21 3Z',
    ];
    g.save(); g.translate(x, y); g.rotate(rotation); g.scale(size / 28, size / 28 * .64);
    const shape = new Path2D(shapes[type]);
    g.save(); g.translate(1.7, 2.4); g.globalAlpha = .18; g.fillStyle = '#a36c1b'; g.fill(shape); g.restore();
    g.fillStyle = color; g.fill(shape);
    g.save(); g.clip(shape);
    for (let i = 0; i < 180; i++) {
      g.fillStyle = i % 3 ? 'rgba(244,180,75,.25)' : 'rgba(117,70,24,.16)';
      g.fillRect(range(type === 3 ? -22 : -14, type === 3 ? 22 : 14), range(-15, 14), range(.4, 1.3), range(.4, 1.1));
    }
    g.restore(); g.strokeStyle = 'rgba(116,72,29,.54)'; g.lineWidth = .65; g.lineCap = 'round';
    g.beginPath();
    if (type === 3) {
      g.strokeStyle = 'rgba(151,95,24,.42)';
      g.moveTo(-26, 7); g.quadraticCurveTo(-8, -2, 19, -1);
      for (const xx of [-12, -4, 5]) {
        g.moveTo(xx, 1); g.lineTo(xx + 4, -3.5);
        g.moveTo(xx, 1); g.lineTo(xx + 6, 5);
      }
    } else {
      g.moveTo(1, 17); g.quadraticCurveTo(-1, 8, .5, -11);
      for (const [side, yy] of [[-1, 2], [1, 0], [-1, -5], [1, -6]]) { g.moveTo(0, yy + 4); g.lineTo(side * 6, yy); }
    }
    g.stroke(); g.restore();
    // A few stems cross each resting leaf, so it belongs to the grass surface.
    g.save(); g.globalAlpha = .85;
    for (let i = 0; i < 3; i++) blade(g, x + range(-size * .35, size * .35), y + size * .23, range(3, size * .42), range(-4, 4), .65, '#edbb58');
    g.restore();
  };
  // Shallow depth bands keep moving blades behind the next row of pumpkins.
  // Only a fifth of the blades move; the remaining paint stays cached.
  const fieldBands = [];
  // Ground cover is distributed independently of the pumpkins. Sorting both
  // by their ground position supplies natural overlap without pale plinths.
  const fieldObjects = [];
  for (const spec of [
    [118, 79, 58, 63, 0, -.22, true, .96], [181, 51, 98, 78, 1, .12], [134, 81, 113, 82, 2, -.11],
    [548, 47, 83, 67, 2, -.15, false, .94], [513, 69, 76, 64, 0, .12, false, .94],
    [188, 200, 104, 90, 2, .15], [407, 222, 101, 83, 2, -.09], [496, 232, 65, 52, 2, .17, true],
  ]) fieldObjects.push({ kind: 'pumpkin', y: spec[1] + spec[3] * .93, spec });
  // The two low orange marks at the reference's side edges are resting leaves.
  for (const spec of [[29, 204, 34, -.08, 3, '#e2a02b'], [721, 181, 40, -.19, 3, '#e7a331']]) {
    fieldObjects.push({ kind: 'leaf', y: spec[1] + 3, spec });
  }
  for (let yy = 14; yy < 383; yy += 16) for (let xx = -12; xx < W + 12; xx += 25) {
    const x = xx + range(-14, 14), y = yy + range(-8, 8);
    const growth = (Math.sin(x * .026 + y * .041) + Math.sin(x * .061 - y * .017) + 2) / 4;
    if (random() < .06 + (1 - growth) * .14) continue;
    const depth = .58 + y / H * .78;
    fieldObjects.push({ kind: 'tuft', y, spec: [x, y, range(23, 40) * depth, range(16, 32) * depth, Math.round(36 + growth * 50), growth > .52] });
  }
  for (const spec of [[324, 63, 13, -.6, 2, '#bd8128'], [664, 89, 16, .5, 0, '#c77c24'], [295, 123, 14, 1.4, 1, '#b37a2b'], [484, 172, 18, -.8, 0, '#ca862b'], [87, 225, 20, .8, 1, '#b97525'], [617, 244, 19, -.4, 2, '#c78a2a'], [69, 330, 22, -1, 0, '#bd7824']]) {
    fieldObjects.push({ kind: 'leaf', y: spec[1] + 3, spec });
  }
  fieldObjects.sort((a, b) => a.y - b.y);
  let band = null, bandRow = -1;
  for (const object of fieldObjects) {
    const row = Math.floor(object.y / 32);
    // Flush moving grass before every solid object, including objects inside
    // one depth row. Otherwise blades rooted behind a pumpkin cross its crown.
    if (!band || row !== bandRow || object.kind !== 'tuft') {
      const surface = layer(W, 200), y = row * 32 - 140;
      surface.ctx.translate(0, -y);
      band = { surface, y, blades: [], objects: [], windSites: [] }; bandRow = row; fieldBands.push(band);
    }
    band.objects.push(object);
    if (object.kind === 'pumpkin') placePumpkin(band.surface.ctx, ...object.spec);
    else if (object.kind === 'tuft') fieldTuft(band.surface.ctx, ...object.spec, band.blades, band.windSites);
    else if (object.kind === 'leaf') fallenLeaf(band.surface.ctx, ...object.spec);
  }
  // A fill per moving blade is much more expensive than constructing the
  // curves. Keep the original depth order, but fill nearby opacities together
  // within each band. The five stops differ by at most .06 from the source.
  const bladeAlphas = [.38, .5, .62, .74, .84];
  for (const band of fieldBands) {
    // Most bands occupy a narrow strip. Crop their transparent margins once
    // instead of copying a 735×200 surface to the main canvas every frame.
    let left = W, top = H, right = 0, bottom = 0;
    for (const object of band.objects) {
      const [x, y, a, b] = object.spec;
      if (object.kind === 'pumpkin') {
        left = Math.min(left, x - b * .4 - 12);
        top = Math.min(top, y - a * .4 - 12);
        right = Math.max(right, x + a + b * .4 + 12);
        bottom = Math.max(bottom, y + b + a * .4 + 12);
      } else if (object.kind === 'leaf') {
        left = Math.min(left, x - a - 12); top = Math.min(top, y - a - 12);
        right = Math.max(right, x + a + 12); bottom = Math.max(bottom, y + a + 12);
      } else {
        left = Math.min(left, x - a - b - 12); top = Math.min(top, y - b - 12);
        right = Math.max(right, x + a + b + 12); bottom = Math.max(bottom, y + 12);
      }
    }
    const source = band.surface;
    const sx = Math.max(0, Math.floor(left * SCALE));
    const sy = Math.max(0, Math.floor((top - band.y) * SCALE));
    const ex = Math.min(source.canvas.width, Math.ceil(right * SCALE));
    const ey = Math.min(source.canvas.height, Math.ceil((bottom - band.y) * SCALE));
    const cropped = layer((ex - sx) / SCALE, (ey - sy) / SCALE);
    cropped.ctx.drawImage(source.canvas, sx, sy, ex - sx, ey - sy, 0, 0, cropped.w, cropped.h);
    band.surface = cropped;
    band.x = sx / SCALE; band.y += sy / SCALE;
    source.canvas.width = source.canvas.height = 1;
    delete band.objects;
    const batches = new Map();
    for (const blade of band.blades) {
      const alphaIndex = Math.max(0, Math.min(bladeAlphas.length - 1, Math.round((blade.alpha - .38) / .12)));
      const color = blade.spec[5], key = `${color}:${alphaIndex}`;
      if (!batches.has(key)) batches.set(key, { color, alpha: bladeAlphas[alphaIndex], blades: [] });
      batches.get(key).blades.push({ spec: blade.spec, windSite: blade.windSite });
    }
    band.batches = [...batches.values()];
    delete band.blades;
  }

  const cat = layer(235, 310), catG = cat.ctx, headGrain = layer(235, 310);
  catG.translate(-235, -117);
  const bodyPath = new Path2D('M302 258 C320 250 365 254 386 273 C394 294 411 325 425 355 C445 398 408 419 347 419 C291 419 266 395 273 355 C279 321 286 288 302 258Z');
  catG.fillStyle = ink; catG.fill(bodyPath);
  headGrain.ctx.translate(-235, -117);
  for (let i = 0; i < 9500; i++) {
    headGrain.ctx.fillStyle = i % 2 ? 'rgba(146,145,109,.10)' : 'rgba(0,0,0,.15)';
    headGrain.ctx.fillRect(range(267, 437), range(133, 299), range(.35, 1.1), range(.35, 1.3));
  }
  catG.save(); catG.clip(bodyPath);
  for (let i = 0; i < 3900; i++) { catG.fillStyle = 'rgba(161,154,112,.07)'; catG.fillRect(range(271, 435), range(255, 422), .6, .9); }
  catG.restore();

  const frontPumpkins = layer();
  placePumpkin(frontPumpkins.ctx, 136, 239, 131, 132, 0, -.16, true);
  placePumpkin(frontPumpkins.ctx, 180, 289, 128, 104, 2, -.07);
  placePumpkin(frontPumpkins.ctx, 476, 256, 126, 124, 1, .035);
  placePumpkin(frontPumpkins.ctx, 408, 306, 100, 79, 2, -.15);
  placePumpkin(frontPumpkins.ctx, 72, 322, 82, 55, 2, -.12, false, .85);

  const edgeKnots = [[0, 365], [90, 375], [165, 372], [235, 383], [278, 367], [314, 351], [344, 357], [377, 350], [414, 371], [453, 382], [490, 378], [537, 374], [594, 365], [655, 358], [735, 370]];
  const groundEdge = x => {
    const i = Math.max(0, edgeKnots.findIndex(p => p[0] >= x) - 1);
    const a = edgeKnots[i], b = edgeKnots[i + 1], u = Math.max(0, Math.min(1, (x - a[0]) / (b[0] - a[0])));
    const easing = u * u * (3 - 2 * u);
    return a[1] * (1 - easing) + b[1] * easing + Math.sin(x * .32) * 1.5 + Math.sin(x * .13) * 2;
  };
  const ground = layer();
  const cover = new Path2D(); cover.moveTo(0, H);
  for (let x = 0; x <= W; x += 1) cover.lineTo(x, groundEdge(x));
  cover.lineTo(W, H); cover.closePath();
  ground.ctx.fillStyle = '#f7c35b'; ground.ctx.fill(cover);
  ground.ctx.save(); ground.ctx.clip(cover);
  // Broad ochre hollows sit beneath the illuminated grass, with smaller warm
  // contact shadows where the pumpkins disappear into the bank.
  for (const [x, y, rx, ry, color] of [
    [140, 414, 70, 22, '#df941f'], [342, 428, 86, 30, '#df941e'],
    [494, 408, 91, 28, '#e39824'], [623, 450, 65, 37, '#e29b25'],
    [61, 471, 61, 32, '#e9a129'], [432, 505, 78, 25, '#e49a21'],
    [206, 379, 45, 12, '#d58a1d'], [457, 381, 43, 11, '#d28419'],
    [541, 375, 37, 12, '#d58c20'],
  ]) dryPatch(ground.ctx, x, y, rx, ry, color, 1.25);
  for (let i = 0; i < 45; i++) dryPatch(ground.ctx, range(0, W), range(370, 545), range(20, 65), range(8, 25), random() < .45 ? '#e8a532' : '#ffd578', .7);
  grass(ground.ctx, 24000, 0, W, 355, 533, 2, 14, ['#ffd57b', '#f4c25f', '#dda338', '#db982d'], .62);
  for (const [x, y, w] of [[126, 410, 107], [330, 422, 110], [488, 402, 109], [606, 443, 70]]) {
    grass(ground.ctx, 450, x - w / 2, x + w / 2, y - 8, y + 5, 4, 17, ['#fbd071', '#ffda7a', '#efb743'], .9);
  }
  ground.ctx.restore();
  for (let i = 0; i < 13500; i++) {
    const x = range(0, W), y = groundEdge(x) + range(0, 15), h = range(4, 19);
    ground.ctx.globalAlpha = range(.45, 1);
    blade(ground.ctx, x, y, h, range(-h * .6, h * .6), range(.4, 1.3), ['#ffd475', '#f7c35b', '#ffda80'][i % 3]);
  }
  ground.ctx.globalAlpha = 1;
  for (const spec of [[136, 405, 26, .65, 0, '#bc7425'], [374, 400, 22, -.8, 2, '#c48125'], [506, 450, 30, -1.05, 1, '#ae6925'], [169, 479, 25, .35, 0, '#c7892f'], [637, 402, 24, 1.1, 1, '#bc7e2a'], [255, 446, 20, -.2, 2, '#d19330'], [450, 476, 17, .4, 2, '#c38429']]) {
    fallenLeaf(ground.ctx, ...spec);
  }
  const frontStalks = layer();
  // Sparse background blades and opaque, broad foreground leaves must read as
  // separate depth planes. The reference's tall seed heads anchor the corners.
  grass(frontStalks.ctx, 45, -25, 160, 487, 542, 25, 106, ['#d88c1d', '#db9624', '#e4a331'], .9, 2);
  grass(frontStalks.ctx, 45, 525, 779, 485, 543, 27, 112, ['#d58b1d', '#e09b25', '#e6a22b'], .9, 2);
  const reedClump = (g, x, y, height, spread, count) => {
    g.save();
    for (let i = 0; i < count; i++) {
      const fan = (i / (count - 1) * 2 - 1), lean = fan * spread + range(-9, 9);
      const h = height * range(.48, 1) * (1 - Math.abs(fan) * .18);
      g.globalAlpha = range(.74, .98);
      blade(g, x + range(-7, 7), y + range(-3, 9), h, lean, range(2.7, 6.2), ['#d18417', '#d9901d', '#e39922', '#cf8016'][i % 4]);
    }
    g.restore();
  };
  const seedStalk = (g, x, y, height, lean, headLength, headWidth) => {
    const tipX = x + lean, tipY = y - height;
    g.save(); g.strokeStyle = '#d78b1a'; g.lineWidth = 1.8; g.lineCap = 'round';
    g.beginPath(); g.moveTo(x, y); g.quadraticCurveTo(x + lean * .2, y - height * .48, tipX, tipY + 2); g.stroke();
    g.save(); g.translate(tipX, tipY); g.rotate(Math.atan2(lean * .8, height));
    const head = new Path2D();
    head.moveTo(0, -2);
    head.bezierCurveTo(-headWidth * .45, headLength * .13, -headWidth * .69, headLength * .53, -headWidth * .24, headLength * .85);
    head.quadraticCurveTo(0, headLength + 3, headWidth * .29, headLength * .78);
    head.bezierCurveTo(headWidth * .73, headLength * .49, headWidth * .4, headLength * .09, 0, -2);
    g.fillStyle = '#d28616'; g.fill(head);
    g.save(); g.clip(head);
    for (let i = 0; i < 130; i++) {
      g.fillStyle = i % 3 ? 'rgba(174,99,12,.23)' : 'rgba(247,173,44,.5)';
      g.fillRect(range(-headWidth, headWidth), range(0, headLength), range(.3, 1), range(.5, 1.7));
    }
    g.restore();
    g.strokeStyle = 'rgba(204,126,17,.65)'; g.lineWidth = .65;
    for (let i = 0; i < 14; i++) {
      const t = (i + .5) / 14, side = i % 2 ? 1 : -1;
      const sideX = Math.sin(t * Math.PI) * headWidth * .45 * side;
      g.beginPath(); g.moveTo(sideX, t * headLength); g.lineTo(sideX + side * range(.8, 2), t * headLength - range(1.5, 3)); g.stroke();
    }
    g.restore();
    blade(g, x + lean * .08, y - height * .17, height * .46, -height * .22, 3.3, '#d88d1a');
    blade(g, x + lean * .15, y - height * .29, height * .36, height * .19, 3.8, '#dc901c');
    g.restore();
  };
  for (const spec of [[16, 516, 144, 55, 15], [80, 526, 110, 54, 12], [282, 529, 115, 67, 14], [584, 524, 135, 74, 13], [660, 521, 104, 59, 14], [730, 525, 142, 53, 14]]) {
    reedClump(frontStalks.ctx, ...spec);
  }
  for (const spec of [[36, 519, 158, -12, 32, 5.3], [79, 526, 120, -27, 24, 4.5], [299, 527, 105, -22, 22, 3.8], [571, 521, 150, -25, 34, 6.2], [693, 524, 113, 20, 25, 4.8], [737, 521, 128, -15, 29, 5]]) {
    seedStalk(frontStalks.ctx, ...spec);
  }
  frontStalks.ctx.save(); frontStalks.ctx.globalCompositeOperation = 'destination-out';
  frontStalks.ctx.fillStyle = 'rgba(0,0,0,.19)';
  for (let i = 0; i < 14000; i++) frontStalks.ctx.fillRect(range(0, W), range(360, H), range(.3, .8), range(.3, 1));
  frontStalks.ctx.restore();

  const movingGrass = [];
  for (let i = 0; i < 46; i++) {
    const x = range(115, 610), y = groundEdge(x) + range(2, 9), phase = range(0, Math.PI * 2);
    const blades = [];
    for (let j = 0; j < 8; j++) blades.push({ x: range(-7, 7), height: range(9, 24), lean: range(-10, 10), width: range(.55, 1.05), color: random() < .5 ? '#ffda80' : '#f7c35b' });
    movingGrass.push({ x, y, phase, blades });
  }
  const tailAngles = [-.2, -.43, -.68, -.86, -1.05, -1.25, -1.45, -1.65, -1.84, -2.02, -2.15];
  const tailLengths = tailAngles.map((_, i) => i < 2 ? 20 : 14.8);
  const tailGrain = Array.from({ length: 650 }, () => ({ u: random(), across: range(-8, 8), light: random() < .5, radius: range(.25, .6) }));
  const smooth = t => { const n = Math.max(0, Math.min(1, t)); return n * n * (3 - 2 * n); };
  const blinkPulse = t => t < 0 || t > .36 ? 0 : t < .11 ? smooth(t / .11) : t < .16 ? 1 : 1 - smooth((t - .16) / .20);
  const earPulse = t => {
    if (t < 0 || t > 1.6) return 0;
    const ease = u => u * u * u * (u * (u * 6 - 15) + 10);
    if (t < .52) return ease(t / .52);
    if (t < .66) return 1;
    return 1 - ease((t - .66) / .94);
  };
  const earAngles = (time, manual = -20) => [
    -.105 * Math.max(earPulse(time % 14.7 - 3.1), earPulse(manual)),
    .17 * Math.max(earPulse(time % 19.3 - 9.2), .62 * earPulse(manual - .48)),
  ];
  const paintHead = (g, angles) => {
    // Move only the ear control points; the cheek, eyes and ear roots stay put.
    // A single closed outline avoids seams between rotated ear cutouts and face.
    const earPoint = (x, y, side, weight = 1) => {
      const px = side === 0 ? 324 : 398, py = side === 0 ? 181 : 197;
      const a = angles[side] * weight, dx = x - px, dy = y - py;
      return [px + dx * Math.cos(a) - dy * Math.sin(a), py + dx * Math.sin(a) + dy * Math.cos(a)];
    };
    const head = new Path2D(); head.moveTo(274, 241);
    head.bezierCurveTo(270, 225, 282, 204, 295, 184);
    head.bezierCurveTo(...earPoint(307, 164, 0, .55), ...earPoint(315, 141, 0), ...earPoint(326, 137, 0));
    head.bezierCurveTo(...earPoint(337, 133, 0), ...earPoint(343, 151, 0, .85), ...earPoint(351, 164, 0, .45));
    head.quadraticCurveTo(359, 173, 375, 176);
    head.bezierCurveTo(...earPoint(391, 170, 1, .4), ...earPoint(415, 165, 1), ...earPoint(426, 170, 1));
    head.bezierCurveTo(...earPoint(437, 176, 1), ...earPoint(427, 200, 1, .4), 421, 211);
    head.bezierCurveTo(429, 227, 419, 250, 407, 267);
    head.bezierCurveTo(394, 286, 373, 296, 349, 295);
    head.bezierCurveTo(310, 294, 283, 280, 277, 257); head.closePath();
    g.fillStyle = ink; g.fill(head); g.save(); g.clip(head);
    drawLayer(g, headGrain, 235, 117); g.restore();
  };
  const windAt = (time, x, y) => {
    const phase = time * .92 - x * .009 - y * .013;
    const gust = (.5 + .5 * Math.sin(time * .31 - y * .012)) ** 2;
    return Math.sin(phase) * (1.4 + 3.6 * gust) + Math.sin(phase * 1.73 + x * .019) * .5;
  };
  const airLeaves = [0, 1].map(i => {
    const sprite = layer(40, 40), g = sprite.ctx;
    const shape = new Path2D(i ? 'M19 31 C6 23 8 11 22 5 C32 13 32 26 19 31Z' : 'M20 32 L16 25 L9 26 L11 19 L4 15 L13 13 L12 6 L19 10 L23 3 L25 12 L33 10 L29 18 L35 23 L26 24Z');
    g.fillStyle = i ? '#cf8326' : '#b97927'; g.fill(shape);
    g.save(); g.clip(shape);
    for (let j = 0; j < 180; j++) {
      g.fillStyle = j % 2 ? 'rgba(255,202,98,.32)' : 'rgba(141,84,24,.20)';
      g.fillRect(range(5, 34), range(3, 33), .8, 1);
    }
    g.restore(); g.strokeStyle = 'rgba(121,76,26,.55)'; g.lineWidth = .9;
    g.beginPath(); g.moveTo(18, 35); g.quadraticCurveTo(21, 20, 22, 8); g.stroke();
    return sprite;
  });
  const paintAirLeaves = (g, time) => {
    let count = 0;
    for (let i = 0; i < 2; i++) {
      const age = (time + i * 12 + 4) % 29, duration = 15 + i * 2;
      if (age > duration) continue;
      const u = age / duration, x = -40 + u * (W + 80);
      const y = 50 + i * 92 + u * 75 + Math.sin(u * Math.PI * 3 + i) * 24;
      g.save(); g.globalAlpha = .84 * smooth(u * 9) * smooth((1 - u) * 9);
      g.translate(x, y); g.rotate(-.5 + u * 4 + .3 * Math.sin(age * 1.2));
      g.scale(.45 + i * .08, (.45 + i * .08) * (.65 + .35 * Math.cos(age * 1.8)));
      drawLayer(g, airLeaves[i], -20, -20); g.restore(); count++;
    }
    return count;
  };
  const tailPoints = (time, boost) => {
    const points = [{ x: 421, y: 353 }];
    const cycle = time % 9.8;
    const envelope = cycle < 4.5 ? Math.sin(Math.PI * cycle / 4.5) ** 2 : 0;
    for (let i = 0; i < tailAngles.length; i++) {
      const u = i / (tailAngles.length - 1), influence = u * u;
      const angle = tailAngles[i] + influence * (.30 * Math.sin(time * 1.4 - u * .7) * envelope + boost * .36 * Math.sin(time * 2.1 - u));
      const p = points[points.length - 1];
      points.push({ x: p.x + Math.cos(angle) * tailLengths[i], y: p.y + Math.sin(angle) * tailLengths[i] });
    }
    return points;
  };
  const paintTail = (g, points) => {
    const samples = [];
    for (let i = 0; i < points.length - 1; i++) {
      const p0 = points[Math.max(0, i - 1)], p1 = points[i], p2 = points[i + 1], p3 = points[Math.min(points.length - 1, i + 2)];
      for (let k = 0; k < 7; k++) {
        const t = k / 7, t2 = t * t, t3 = t2 * t;
        samples.push({ x: .5 * (2 * p1.x + (-p0.x + p2.x) * t + (2 * p0.x - 5 * p1.x + 4 * p2.x - p3.x) * t2 + (-p0.x + 3 * p1.x - 3 * p2.x + p3.x) * t3), y: .5 * (2 * p1.y + (-p0.y + p2.y) * t + (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 + (-p0.y + 3 * p1.y - 3 * p2.y + p3.y) * t3) });
      }
    }
    samples.push(points[points.length - 1]);
    const outline = new Path2D(), left = [], right = [];
    for (let i = 0; i < samples.length; i++) {
      const a = samples[Math.max(0, i - 1)], b = samples[Math.min(samples.length - 1, i + 1)], p = samples[i];
      const angle = Math.atan2(b.y - a.y, b.x - a.x), u = i / (samples.length - 1);
      const width = 10.9 - u * 1.7 + Math.sin(u * 98) * .13;
      p.nx = -Math.sin(angle); p.ny = Math.cos(angle);
      left.push([p.x + p.nx * width, p.y + p.ny * width]); right.push([p.x - p.nx * width, p.y - p.ny * width]);
    }
    outline.moveTo(...left[0]); for (let i = 1; i < left.length; i++) outline.lineTo(...left[i]);
    const tip = samples[samples.length - 1], tipAngle = Math.atan2(tip.ny, tip.nx);
    outline.arc(tip.x, tip.y, 9.2, tipAngle, tipAngle - Math.PI, true);
    for (let i = right.length - 1; i >= 0; i--) outline.lineTo(...right[i]); outline.closePath();
    g.fillStyle = ink; g.fill(outline); g.save(); g.clip(outline);
    for (const fleck of tailGrain) {
      const p = samples[Math.floor(fleck.u * (samples.length - 1))];
      g.fillStyle = fleck.light ? 'rgba(145,143,110,.13)' : 'rgba(0,0,0,.12)';
      g.fillRect(p.x + p.nx * fleck.across, p.y + p.ny * fleck.across, fleck.radius, fleck.radius);
    }
    g.restore();
  };
  const eye = (g, x, y, closure, rotation) => {
    g.save(); g.translate(x, y); g.rotate(rotation);
    const open = 1 - closure, rx = 12.3 - closure * .6, ry = 12.9 * open;
    g.strokeStyle = '#f4f3e8'; g.lineWidth = 4.25; g.lineCap = 'round';
    g.beginPath(); g.moveTo(-rx, 0); g.bezierCurveTo(-rx, -ry * 1.3, rx, -ry * 1.3, rx, 0); g.bezierCurveTo(rx, ry * 1.3 + closure * 3, -rx, ry * 1.3 + closure * 3, -rx, 0); g.stroke();
    g.restore();
  };
  // Cached light masks change the broad composition without softening any of
  // the grass texture. Pigment grain is spatially fixed, never regenerated.
  // These are broad, blurred light masks. One source pixel per scene unit is
  // sufficient even when the detailed painting uses a denser backing store.
  const maskScale = 1, maskWidth = W, maskHeight = H;
  const canopyShade = layer(W, H, maskScale), sunBreaks = layer(W, H, maskScale);
  const shadePixels = canopyShade.ctx.createImageData(maskWidth, maskHeight);
  const sunPixels = sunBreaks.ctx.createImageData(maskWidth, maskHeight);
  const shadeForms = [
    [118, 65, 238, 43, -.33, 1],
    [538, 154, 271, 48, -.36, .92],
    [11, 290, 163, 32, -.39, .65],
    [698, 304, 152, 38, -.42, .6],
  ].map(([x, y, length, width, angle, strength]) => ({ x, y, length, width, c: Math.cos(angle), s: Math.sin(angle), strength }));
  const lightForms = [[348, 57, 165, 54], [174, 202, 141, 48], [553, 291, 181, 53]];
  const paintNoise = (x, y) => {
    const gx = x / 16, gy = y / 16, ix = Math.floor(gx), iy = Math.floor(gy);
    const tx = smooth(gx - ix), ty = smooth(gy - iy), i = iy * coarseW + ix;
    return (coarse[i] * (1 - tx) + coarse[i + 1] * tx) * (1 - ty) + (coarse[i + coarseW] * (1 - tx) + coarse[i + coarseW + 1] * tx) * ty;
  };
  for (let py = 0; py < maskHeight; py++) for (let px = 0; px < maskWidth; px++) {
    const x = px / maskScale, y = py / maskScale, p = (py * maskWidth + px) * 4;
    const noise = paintNoise(x, y), rough = (noise - .5) * 15;
    let shade = 0, sunlight = 0;
    for (const form of shadeForms) {
      const dx = x - form.x, dy = y - form.y;
      const along = (dx * form.c + dy * form.s) / form.length;
      const cross = -dx * form.s + dy * form.c;
      const width = form.width * (1 + .14 * Math.sin(along * 13) + .08 * Math.cos(along * 23));
      const edge = (Math.abs(cross + rough) / width) ** 2 + along ** 4;
      shade = Math.max(shade, (1 - smooth((edge - .45) / .72)) * form.strength);
    }
    for (const [cx, cy, rx, ry] of lightForms) {
      const dx = x - cx, dy = y - cy;
      const along = (dx * .94 - dy * .34) / rx, cross = (dx * .34 + dy * .94 + rough * .6) / ry;
      sunlight = Math.max(sunlight, 1 - smooth(along * along + cross * cross));
    }
    // Keep the cat's surrounding clearing luminous and fade into the lit bank.
    const clearing = 1 - smooth(((x - 351) / 146) ** 2 + ((y - 292) / 136) ** 2);
    const distanceFade = 1 - smooth((y - 318) / 67);
    let hash = Math.imul(px + 19, 374761393) ^ Math.imul(py + 71, 668265263);
    hash = Math.imul(hash ^ hash >>> 13, 1274126177);
    const grain = (hash >>> 0) / 4294967296;
    const pigment = .73 + grain * .35 + noise * .17;
    shadePixels.data[p] = 151; shadePixels.data[p + 1] = 105; shadePixels.data[p + 2] = 53;
    shadePixels.data[p + 3] = Math.round(255 * .34 * shade * pigment * (1 - clearing * .8) * distanceFade);
    sunPixels.data[p] = 255; sunPixels.data[p + 1] = 214; sunPixels.data[p + 2] = 123;
    sunPixels.data[p + 3] = Math.round(255 * .12 * sunlight * pigment * (1 - shade) * distanceFade);
  }
  canopyShade.ctx.putImageData(shadePixels, 0, 0); sunBreaks.ctx.putImageData(sunPixels, 0, 0);
  const framingBranch = layer(W, 145), branchG = framingBranch.ctx;
  const branchOutline = new Path2D('M755 -12 C720 8 684 17 651 18 C609 14 575 12 545 18 Q514 24 481 39 Q517 29 546 24 C579 20 609 23 650 26 C691 25 727 17 756 4Z');
  branchG.fillStyle = '#956331'; branchG.fill(branchOutline);
  branchG.save(); branchG.clip(branchOutline);
  for (let i = 0; i < 1700; i++) {
    branchG.fillStyle = i % 3 ? 'rgba(224,162,77,.32)' : 'rgba(98,58,25,.2)';
    branchG.fillRect(range(480, 755), range(-12, 42), range(.4, 2.1), range(.3, .9));
  }
  branchG.restore(); branchG.lineCap = 'round';
  for (const [path, width] of [
    ['M704 14 Q716 41 722 67 Q729 89 741 104', 3.2],
    ['M680 21 Q670 32 663 43', 1.8],
    ['M627 21 Q612 8 598 -5', 2.1],
    ['M579 19 Q571 34 555 43', 1.5],
    ['M533 24 Q519 15 512 4', 1.4],
    ['M720 60 Q705 65 700 75', 1.4],
  ]) {
    branchG.strokeStyle = '#a06c34'; branchG.lineWidth = width; branchG.stroke(new Path2D(path));
  }
  const branchLeaves = ['#bf7d29', '#d19437', '#bd722c'].map((color, variant) => {
    const sprite = layer(68, 80), g = sprite.ctx; g.translate(34, 12);
    const outline = new Path2D(variant === 1
      ? 'M0 0 C-4 4 -13 2 -10 11 C-20 12 -15 22 -10 22 C-16 30 -7 35 -3 34 L2 45 Q6 38 8 34 C18 34 20 25 12 23 C21 17 16 10 10 11 Q12 3 0 0Z'
      : 'M0 0 Q-5 4 -10 0 L-10 10 L-21 8 L-15 20 L-23 23 L-10 29 L-11 37 L-3 34 L3 46 L7 34 L16 36 L14 27 L25 20 L16 17 L19 7 L8 10 L7 1Z');
    g.fillStyle = color; g.fill(outline); g.save(); g.clip(outline);
    for (let i = 0; i < 1500; i++) {
      g.fillStyle = i % 3 ? 'rgba(247,191,93,.28)' : 'rgba(126,72,24,.17)';
      g.fillRect(range(-25, 26), range(-2, 47), range(.35, 1.25), range(.4, 1.5));
    }
    g.restore(); g.lineCap = 'round';
    g.strokeStyle = 'rgba(122,78,31,.54)'; g.lineWidth = .85;
    g.beginPath(); g.moveTo(0, -6); g.bezierCurveTo(-2, 8, 4, 29, 3, 42);
    for (const [x, y, endY] of [[-10, 9, 16], [12, 14, 21], [-11, 24, 29], [11, 29, 34]]) {
      g.moveTo(x, y); g.quadraticCurveTo(x * .4, y + 2, 2, endY);
    }
    g.stroke(); return sprite;
  });
  const branchLeafPlacements = [
    [489, 37, 37, .72, 1], [513, 5, 42, 2.52, 0],
    [555, 43, 43, .32, 0], [598, -3, 47, 2.6, 1],
    [624, 21, 34, -.9, 2], [663, 43, 49, -.43, 1],
    [686, 19, 47, 2.48, 0], [700, 75, 41, .29, 2],
    [741, 104, 46, -.34, 0], [728, 37, 43, -1.42, 1],
  ];
  const paintFramingBranch = (g, time, wind, gust = 0) => {
    g.save(); g.translate(W, 0); g.rotate(gust * .0035); g.translate(-W, 0);
    drawLayer(g, framingBranch);
    for (const [x, y, size, angle, variant] of branchLeafPlacements) {
      const sway = wind * .025 * Math.sin(time * .66 + x * .022 + y * .017) + gust * .035;
      g.save(); g.translate(x, y); g.rotate(angle + sway); g.scale(size / 46, size / 46);
      drawLayer(g, branchLeaves[variant], -34, -6); g.restore();
    }
    g.restore();
  };
  let disposed = false;
  let backgroundCached = false;
  const render = (time, options = {}) => {
    if (disposed) return;
    const started = performance.now();
    ctx.setTransform(SCALE, 0, 0, SCALE, 0, 0); ctx.globalAlpha = 1; ctx.globalCompositeOperation = 'source-over';
    const windTime = options.windTime ?? time, windStrength = options.wind ?? 1;
    const cursorWind = x => {
      const direction = options.gustDirection ?? 1;
      const delay = (direction > 0 ? x / W : 1 - x / W) * .65;
      const phase = ((options.gustTime ?? -20) - delay) / 2.4;
      return phase > 0 && phase < 1 ? direction * (options.gustStrength ?? 0) * 7 * Math.sin(Math.PI * phase) ** 2 : 0;
    };
    drawLayer(ctx, field);
    if (!backgroundCached) {
    for (const band of fieldBands) {
      drawLayer(ctx, band.surface, band.x, band.y);
      const winds = band.windSites.map(([x, y]) => windAt(windTime, x, y) * windStrength + cursorWind(x));
      for (const batch of band.batches) {
        const path = new Path2D();
        for (const blade of batch.blades) {
          const [x, y, height, lean, width] = blade.spec;
          traceBlade(path, x, y, height, lean + winds[blade.windSite] * height / 23, width);
        }
        ctx.globalAlpha = batch.alpha;
        ctx.fillStyle = batch.color;
        ctx.fill(path);
      }
      ctx.globalAlpha = 1;
    }
    ctx.globalCompositeOperation = 'multiply'; drawLayer(ctx, canopyShade);
    ctx.globalCompositeOperation = 'screen'; drawLayer(ctx, sunBreaks);
    ctx.globalCompositeOperation = 'source-over';
    if (configuration.cacheBackground) {
      // Distant blades and the fixed light masks become one opaque texture.
      // The cat, airborne leaves, foreground grass and branch remain animated.
      field.ctx.save(); field.ctx.setTransform(1, 0, 0, 1, 0, 0);
      field.ctx.drawImage(canvas, 0, 0); field.ctx.restore();
      for (const band of fieldBands) band.surface.canvas.width = band.surface.canvas.height = 1;
      fieldBands.length = 0;
      canopyShade.canvas.width = canopyShade.canvas.height = 1;
      sunBreaks.canvas.width = sunBreaks.canvas.height = 1;
      backgroundCached = true;
      diagnostics.surfaceBytes = String(4 * (pixelWidth * pixelHeight + surfaces.reduce((sum, l) => sum + l.canvas.width * l.canvas.height, 0)));
    }
    }
    const airborne = paintAirLeaves(ctx, windTime);
    const petTime = options.petTime ?? -20;
    const pet = petTime < 0 || petTime > 2.6 ? 0 : petTime < .48 ? smooth(petTime / .48) : petTime < 1.45 ? 1 : 1 - smooth((petTime - 1.45) / 1.15);
    const attention = earPulse(options.attentionTime ?? -20);
    const points = tailPoints(time, Math.max(options.tailBoost || 0, pet * .32, attention * .26));
    paintTail(ctx, points); drawLayer(ctx, cat, 235, 117);
    const ears = earAngles(time, options.earTime);
    const side = options.attentionSide === 1 ? 1 : 0;
    ears[side] += (side === 0 ? -.12 : .15) * attention;
    paintHead(ctx, ears);
    const cycle = time % 8.9;
    const closure = Math.max(blinkPulse(cycle - 1.7), .8 * blinkPulse(cycle - 6.5), blinkPulse(options.blinkTime ?? -1), pet * .94);
    eye(ctx, 311, 214, closure, .15); eye(ctx, 367, 229, closure, .19);
    drawLayer(ctx, frontPumpkins); drawLayer(ctx, ground);
    for (const tuft of movingGrass) {
      const wind = windStrength * windAt(windTime, tuft.x, tuft.y) + cursorWind(tuft.x);
      for (const b of tuft.blades) blade(ctx, tuft.x + b.x, tuft.y, b.height, b.lean + wind * (b.height / 24), b.width, b.color);
    }
    drawLayer(ctx, frontStalks);
    paintFramingBranch(ctx, windTime, windStrength, cursorWind(W * .8));
    ctx.globalCompositeOperation = 'soft-light'; ctx.globalAlpha = .43; drawLayer(ctx, paper); ctx.globalAlpha = 1; ctx.globalCompositeOperation = 'source-over';
    diagnostics.time = time.toFixed(3); diagnostics.blink = closure.toFixed(3);
    diagnostics.tailTip = `${points.at(-1).x.toFixed(2)},${points.at(-1).y.toFixed(2)}`;
    diagnostics.ears = ears.map(a => a.toFixed(3)).join(',');
    diagnostics.pet = pet.toFixed(3);
    diagnostics.attention = attention.toFixed(3);
    diagnostics.attentionSide = String(side);
    diagnostics.cursorWind = cursorWind(W * .5).toFixed(3);
    diagnostics.windTime = windTime.toFixed(3); diagnostics.airLeaves = String(airborne);
    diagnostics.renderMs = (performance.now() - started).toFixed(2);
  };
  const dispose = () => { disposed = true; for (const l of [paper, field, ...fieldBands.map(b => b.surface), cat, headGrain, frontPumpkins, ground, frontStalks, ...oranges, ivory, ...airLeaves, canopyShade, sunBreaks, framingBranch, ...branchLeaves]) { l.canvas.width = l.canvas.height = 1; } };
  diagnostics.buildMs = (performance.now() - buildStarted).toFixed(1);
  diagnostics.renderScale = String(SCALE);
  const surfaces = [paper, field, ...fieldBands.map(b => b.surface), cat, headGrain, frontPumpkins, ground, frontStalks, ...oranges, ivory, ...airLeaves, canopyShade, sunBreaks, framingBranch, ...branchLeaves];
  diagnostics.surfaceBytes = String(4 * (pixelWidth * pixelHeight + surfaces.reduce((sum, l) => sum + l.canvas.width * l.canvas.height, 0)));
  return { render, dispose, diagnostics, dimensions: { width: W, height: H }, tailPoints, tailLengths };
}
