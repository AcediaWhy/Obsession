type PotionMood = 'rest' | 'brew' | 'ready';
export interface PotionFrame {
  bubbles: { x: number; y: number; radius: number; alpha: number; pop: boolean }[];
  glow: number;
  glint: number;
  puff: number;
}
const wrap = (value: number, period: number) => ((value % period) + period) % period;

export function potionFrameAt(time: number, mood: PotionMood, enabled = true): PotionFrame {
  if (!enabled) return { bubbles: [], glow: 0, glint: -1, puff: -1 };
  const t = wrap(time, 23000);
  const reaction = [4300, 12500, 20100].map(start => t - start).find(age => age >= 0 && age < 1800);
  const brewing = mood === 'brew';
  const bubbles = brewing ? [
    { x: 562, bottom: 1018, radius: 10, offset: 0, period: 2875 },
    { x: 636, bottom: 1050, radius: 14, offset: 1100, period: 3833.3333333333335 },
    { x: 683, bottom: 1015, radius: 9, offset: 1900, period: 3285.714285714286 },
    { x: 598, bottom: 1036, radius: 8, offset: 500, period: 4600 },
  ].map(seed => {
    const phase = wrap(t + seed.offset, seed.period) / seed.period;
    const rise = Math.min(1, phase / .82);
    const pop = phase > .82;
    return { x: seed.x + Math.round(Math.sin(rise * Math.PI * 2) * 5), y: seed.bottom + (896 - seed.bottom) * rise,
      radius: seed.radius + (pop ? (phase - .82) * 70 : 0), pop,
      alpha: phase < .12 ? phase / .12 * .72 : pop ? Math.max(0, 1 - (phase - .82) / .12) * .72 : .72 };
  }) : [];
  const glintAge = wrap(t - 2200, 11500);
  const glint = mood !== 'rest' && glintAge < 1000 ? glintAge / 1000 : -1;
  const pulse = brewing && reaction !== undefined ? Math.sin(Math.min(1, reaction / 1400) * Math.PI) * .18 : 0;
  return { bubbles, glow: (mood === 'rest' ? .025 : .045) + pulse, glint,
    puff: brewing && reaction !== undefined && reaction >= 450 && reaction < 1400 ? (reaction - 450) / 950 : -1 };
}

// Source-space masks stay inside the flask, away from paws and black outlines.
export function drawPotionFrame(canvas: HTMLCanvasElement, frame: PotionFrame, pixels: number) {
  if (canvas.width !== pixels || canvas.height !== pixels) { canvas.width = pixels; canvas.height = pixels; }
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.clearRect(0, 0, pixels, pixels);
  ctx.imageSmoothingEnabled = false;
  const scale = pixels / 1254;
  const p = (n: number) => Math.round(n * scale);
  const rect = (x: number, y: number, w: number, h: number) => ctx.fillRect(p(x), p(y), Math.max(1, p(x + w) - p(x)), Math.max(1, p(y + h) - p(y)));
  const mask = (points: number[][]) => {
    ctx.beginPath();
    points.forEach(([x, y], i) => i ? ctx.lineTo(p(x), p(y)) : ctx.moveTo(p(x), p(y)));
    ctx.closePath(); ctx.clip();
  };
  ctx.save();
  mask([[544, 883], [705, 883], [705, 970], [730, 1002], [730, 1027], [709, 1051], [680, 1070], [566, 1070], [537, 1051], [516, 1027], [516, 1002], [538, 970]]);
  ctx.globalAlpha = frame.glow; ctx.fillStyle = '#d9ff99'; rect(510, 880, 230, 205);
  for (const b of frame.bubbles) {
    if (b.alpha <= 0) continue;
    ctx.globalAlpha = b.alpha; ctx.fillStyle = '#e1ffb6';
    const r = b.radius, line = b.pop ? 3 : 4;
    rect(b.x - r / 2, b.y - r, r, line);
    if (!b.pop) {
      rect(b.x - r / 2, b.y + r - line, r, line);
      rect(b.x - r, b.y - r / 2, line, r);
      rect(b.x + r - line, b.y - r / 2, line, r);
    } else {
      rect(b.x - r, b.y + 2, line, line);
      rect(b.x + r - line, b.y + 2, line, line);
    }
  }
  ctx.restore();
  if (frame.glint >= 0) {
    ctx.save();
    mask([[587, 795], [656, 795], [656, 829], [687, 853], [704, 872], [704, 970], [724, 1004], [707, 1045], [680, 1065], [568, 1065], [536, 1040], [518, 1011], [537, 973], [537, 874], [582, 832]]);
    ctx.fillStyle = '#f4ffe3'; ctx.globalAlpha = Math.sin(frame.glint * Math.PI) * .28;
    const x = 440 + frame.glint * 310;
    for (let row = 0; row < 28; row++) rect(x + row * 4, 790 + row * 10, 12, 10);
    ctx.restore();
  }
  if (frame.puff >= 0) {
    ctx.save(); ctx.fillStyle = '#d9efc0';
    for (let i = 0; i < 3; i++) {
      const age = frame.puff - i * .12;
      if (age < 0) continue;
      ctx.globalAlpha = Math.sin(Math.min(1, age) * Math.PI) * .65;
      const x = 624 + (i - 1) * (8 + age * 15), y = 755 - age * 65;
      rect(x - 5, y - 8, 10, 16); rect(x - 9, y - 4, 18, 8);
    }
    ctx.restore();
  }
}
