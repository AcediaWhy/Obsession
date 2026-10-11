// Dusk changes pigment and light, leaving the meadow's drawing and depth intact.
// Texture grading and soft light maps are built once, never in the frame loop.
export function createMeadowDusk(layer) {
  const lanterns = [
    { x: 134, y: 81, w: 113, h: 82, angle: -.11, phase: 1.6, front: false },
    { x: 180, y: 289, w: 128, h: 104, angle: -.07, phase: 0, front: true },
    { x: 476, y: 256, w: 126, h: 124, angle: .035, phase: 2.4, front: true },
  ];
  for (const lamp of lanterns) {
    const offset = lamp.h * (110 / 150 - .6);
    lamp.mouthX = lamp.x + lamp.w / 2 - Math.sin(lamp.angle) * offset;
    lamp.mouthY = lamp.y + lamp.h * .6 + Math.cos(lamp.angle) * offset;
  }
  const groundLight = (x, y) => {
    let light = 0;
    for (const lamp of lanterns) {
      if (!lamp.front) continue;
      const dx = x - lamp.mouthX, dy = y - lamp.mouthY;
      const forward = Math.max(0, Math.min(1, (dy + 7) / 20));
      const aperture = 24 + Math.max(0, dy) * .52;
      const beam = Math.exp(-((dx / aperture) ** 4) * .65);
      const falloff = (1 + (dx / 67) ** 2 + (dy / 60) ** 2) ** -1.65;
      light += forward * beam * falloff;
    }
    return Math.min(1, light);
  };
  const tone = Float32Array.from({ length: 256 }, (_, i) => Math.max(0, Math.min(1, (i / 255 - .32) / .64)) ** 1.1);
  const colorCache = new Map();
  const channels = (r, g, b, exposure = 1) => {
    const light = tone[Math.round(r * .2126 + g * .7152 + b * .0722)];
    const ember = Math.max(0, Math.min(1, (r - g - 52) / 100));
    const ivory = Math.max(0, 1 - (r - b) / 80) * light;
    return [
      (20 + light * 118 + ember * 90 + ivory * 5) * exposure,
      (18 + light * 93 - ember * 17 + ivory * 14) * exposure,
      (33 + light * 104 - ember * 150 - ivory * 12) * exposure,
    ].map(value => Math.max(0, Math.min(255, Math.round(value))));
  };
  const grade = (surface, exposure = 1) => {
    const { canvas, ctx } = surface;
    const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height), data = pixels.data;
    for (let i = 0; i < data.length; i += 4) {
      if (!data[i + 3]) continue;
      const light = tone[Math.round(data[i] * .2126 + data[i + 1] * .7152 + data[i + 2] * .0722)];
      const ember = Math.max(0, Math.min(1, (data[i] - data[i + 1] - 52) / 100));
      const ivory = Math.max(0, 1 - (data[i] - data[i + 2]) / 80) * light;
      data[i] = (20 + light * 118 + ember * 90 + ivory * 5) * exposure;
      data[i + 1] = (18 + light * 93 - ember * 17 + ivory * 14) * exposure;
      data[i + 2] = (33 + light * 104 - ember * 150 - ivory * 12) * exposure;
    }
    ctx.putImageData(pixels, 0, 0);
  };
  const color = (hex, exposure = 1) => {
    const key = `${hex}:${exposure}`;
    if (!colorCache.has(key)) {
      const rgb = parseInt(hex.slice(1), 16);
      colorCache.set(key, `rgb(${channels(rgb >> 16, rgb >> 8 & 255, rgb & 255, exposure).join(',')})`);
    }
    return colorCache.get(key);
  };
  const pool = (g, x, y, rx, ry, stops) => {
    g.save(); g.translate(x, y); g.scale(rx, ry);
    const gradient = g.createRadialGradient(0, 0, 0, 0, 0, 1);
    for (const [offset, pigment] of stops) gradient.addColorStop(offset, pigment);
    g.fillStyle = gradient; g.fillRect(-1, -1, 2, 2); g.restore();
  };
  const clearing = layer(735, 505, 1);
  pool(clearing.ctx, 331, 193, 420, 295, [
    [0, 'rgba(159,171,213,.085)'], [.55, 'rgba(134,133,179,.03)'], [1, 'rgba(134,133,179,0)'],
  ]);
  pool(clearing.ctx, 187, 139, 66, 43, [[0, 'rgba(235,128,34,.2)'], [.4, 'rgba(225,100,29,.08)'], [1, 'rgba(225,100,29,0)']]);
  const spill = layer(), contact = layer(), pumpkinGlow = layer();
  const prepareGround = ground => {
    const w = spill.canvas.width, h = spill.canvas.height;
    const pixels = spill.ctx.createImageData(w, h);
    for (let yy = 0; yy < h; yy++) for (let xx = 0; xx < w; xx++) {
      const light = groundLight(xx * 735 / w, yy * 505 / h), i = (yy * w + xx) * 4;
      pixels.data[i] = 255; pixels.data[i + 1] = 160; pixels.data[i + 2] = 58;
      pixels.data[i + 3] = Math.round(light * 255 * .46);
    }
    spill.ctx.putImageData(pixels, 0, 0);
    // Warmth falls on the painted ground and blades, rather than across the air.
    for (const l of [spill, contact]) {
      l.ctx.save();
      if (l === contact) for (const lamp of lanterns.filter(l => l.front)) {
        pool(l.ctx, lamp.x + lamp.w / 2, lamp.y + lamp.h * .83, lamp.w * .34, 12,
          [[0, 'rgba(27,17,30,.55)'], [.35, 'rgba(27,17,30,.3)'], [1, 'rgba(27,17,30,0)']]);
      }
      l.ctx.globalCompositeOperation = 'destination-in';
      l.ctx.drawImage(ground.canvas, 0, 0, 735, 505); l.ctx.restore();
    }
  };
  const preparePumpkins = pumpkins => {
    for (const lamp of lanterns.filter(l => l.front)) {
      pool(pumpkinGlow.ctx, lamp.mouthX, lamp.mouthY - 17, lamp.w * .45, lamp.h * .47,
        [[0, 'rgba(255,122,22,.2)'], [.45, 'rgba(242,104,22,.08)'], [1, 'rgba(242,104,22,0)']]);
    }
    pumpkinGlow.ctx.save(); pumpkinGlow.ctx.globalCompositeOperation = 'destination-in';
    pumpkinGlow.ctx.drawImage(pumpkins.canvas, 0, 0, 735, 505); pumpkinGlow.ctx.restore();
  };
  const shade = layer(735, 505, 1);
  pool(shade.ctx, 351, 251, 510, 341, [
    [0, 'rgba(25,18,38,0)'], [.35, 'rgba(25,18,38,0)'], [.7, 'rgba(25,18,38,.15)'], [1, 'rgba(25,18,38,.48)'],
  ]);
  const face = layer(160, 150), glow = layer(160, 150);
  const cuts = new Path2D('M-33 -5 Q-27 -16 -19 -20 Q-16 -8 -12 -2 Q-22 -6 -33 -5Z M13 -3 Q21 -14 28 -20 Q32 -7 35 -3 Q24 -6 13 -3Z M0 -3 L-6 8 Q0 5 6 8Z M-38 14 Q-24 26 -13 25 L-12 19 L-4 22 L-4 29 Q5 31 12 26 L12 20 L20 18 L20 23 Q30 21 39 12 Q34 39 4 41 Q-26 41 -38 14Z');
  for (const [surface, emission] of [[face, false], [glow, true]]) {
    const g = surface.ctx; g.translate(80, 83);
    const light = g.createRadialGradient(4, 28, 1, 0, 19, 60);
    light.addColorStop(0, '#fffbe0'); light.addColorStop(.24, '#ffeb9e');
    light.addColorStop(.58, '#ffb438'); light.addColorStop(1, '#bd4012');
    g.shadowColor = '#ff8c24'; g.shadowBlur = emission ? 16 : 4;
    g.fillStyle = light; g.fill(cuts);
    g.save(); g.shadowBlur = 0; g.clip(cuts);
    for (const x of [-23, 25]) {
      pool(g, x, -10, 22, 17, [[0, 'rgba(255,239,171,.82)'], [.42, 'rgba(255,205,91,.42)'], [1, 'rgba(255,205,91,0)']]);
    }
    g.restore();
    if (!emission) {
      g.shadowBlur = 0; g.strokeStyle = '#391c23'; g.lineWidth = 2.2; g.stroke(cuts);
      // The offset inner wall exposes the thickness of the carved pumpkin.
      g.save(); g.clip(cuts); g.translate(1.1, 2.3);
      g.strokeStyle = 'rgba(234,109,25,.94)'; g.lineWidth = 4.2; g.stroke(cuts);
      g.translate(-1.5, -4.3); g.strokeStyle = 'rgba(255,182,51,.55)';
      g.lineWidth = 1.3; g.stroke(cuts); g.restore();
    }
  }
  const mist = layer(855, 110, 1);
  for (const [x, y, rx, ry, alpha] of [[84, 64, 144, 24, .08], [262, 53, 167, 20, .11], [445, 61, 156, 27, .085], [623, 45, 169, 20, .1], [801, 55, 150, 25, .065]]) {
    pool(mist.ctx, x, y, rx, ry, [[0, `rgba(200,188,211,${alpha})`], [.5, `rgba(181,175,207,${alpha * .45})`], [1, 'rgba(181,175,207,0)']]);
  }
  const paintLanterns = (g, time, front) => {
    for (const lamp of lanterns) {
      if (lamp.front !== front) continue;
      g.save(); g.translate(lamp.x + lamp.w / 2, lamp.y + lamp.h * .6); g.rotate(lamp.angle);
      const args = [-lamp.w / 2, -lamp.h * .6, lamp.w, lamp.h];
      g.globalAlpha = .94 + .035 * Math.sin(time * 1.7 + lamp.phase);
      g.drawImage(face.canvas, ...args);
      // Several gentle, continuous candle frequencies; no random frame flashes.
      g.globalCompositeOperation = 'screen';
      g.globalAlpha = .28 + .045 * Math.sin(time * 1.7 + lamp.phase) + .025 * Math.sin(time * 3.1 + lamp.phase * 2);
      g.drawImage(glow.canvas, ...args); g.restore();
    }
  };
  const paintLight = (g, front = false, time = 0) => {
    g.save(); g.globalCompositeOperation = 'screen';
    if (front) g.globalAlpha = .94 + .035 * Math.sin(time * 1.3) + .015 * Math.sin(time * 2.1 + .8);
    g.drawImage((front ? spill : clearing).canvas, 0, 0, 735, 505); g.restore();
  };
  const paintPumpkinLight = g => {
    g.save(); g.globalCompositeOperation = 'screen'; g.drawImage(pumpkinGlow.canvas, 0, 0, 735, 505); g.restore();
  };
  const paintContact = g => { g.save(); g.drawImage(contact.canvas, 0, 0, 735, 505); g.restore(); };
  const paintMist = (g, time, front) => {
    g.save(); g.globalAlpha = front ? .42 : .65;
    g.drawImage(mist.canvas, -60 + Math.sin(time * .105 + (front ? 2.1 : 0)) * 22,
      (front ? 397 : 245) + Math.sin(time * .08) * 3); g.restore();
  };
  const paintShade = g => { g.save(); g.drawImage(shade.canvas, 0, 0); g.restore(); };
  return { grade, color, groundLight, prepareGround, preparePumpkins, paintLanterns, paintLight, paintPumpkinLight, paintContact,
    paintMist, paintShade, surfaces: [clearing, spill, contact, pumpkinGlow, shade, face, glow, mist] };
}
