// Curved, overlapping brush shapes for the foreground bank. Most grass is
// painted once; a few tips move above the same dense, rooted undergrowth.
export function createMeadowGrass(layer, groundEdge, lighting) {
  const top = 314, surface = layer(735, 191), g = surface.ctx;
  g.translate(0, -top);
  let seed = 281904;
  const random = () => { seed = Math.imul(seed ^ seed >>> 15, 2246822519); seed = Math.imul(seed ^ seed >>> 13, 3266489917); return ((seed ^= seed >>> 16) >>> 0) / 4294967296; };
  const range = (a, b) => a + (b - a) * random();
  const tufts = [], paint = (g, blade, wind = 0) => {
    const { x, y, height: h, lean, width: w, bend, pigment, light } = blade;
    const tip = x + lean + wind;
    const path = new Path2D(); path.moveTo(x - w * .55, y);
    path.bezierCurveTo(x + lean * .08 - bend * .25, y - h * .3,
      x + lean * .55 + bend + wind * .65, y - h * .8, tip, y - h);
    path.bezierCurveTo(x + lean * .54 + bend + w * .55 + wind * .65, y - h * .67,
      x + lean * .1 + w * .65, y - h * .24, x + w * .55, y); path.closePath();
    const warmth = light * .38;
    const rgb = pigment.map((channel, i) => Math.round(channel + ([239, 163, 75][i] - channel) * warmth));
    const color = rgb.join(',');
    const gradient = g.createLinearGradient(x, y, tip, y - h);
    gradient.addColorStop(0, `rgba(${color},.08)`);
    gradient.addColorStop(.36, `rgba(${color},.72)`);
    gradient.addColorStop(1, `rgba(${color},.92)`);
    g.fillStyle = gradient; g.fill(path);
    // Thin lit edges keep the clumps readable without outlining every blade.
    if (light > .16 && blade.edge) {
      g.strokeStyle = `rgba(255,195,105,${light * .4})`; g.lineWidth = .35;
      g.beginPath(); g.moveTo(x + lean * .28 + w * .4, y - h * .4);
      g.quadraticCurveTo(x + lean * .6 + bend + wind * .65, y - h * .78, tip, y - h); g.stroke();
    }
  };
  const palette = [[95, 77, 103], [111, 91, 119], [121, 101, 127], [86, 69, 96]];
  // Uneven islands of short turf connect the taller edge clumps to the ground.
  for (let i = 0; i < 175; i++) {
    const x = range(5, 730), y = groundEdge(x) + range(4, 108);
    const spread = range(10, 26), height = range(6, 17), count = Math.round(range(23, 42));
    const lean = range(-height * .4, height * .4);
    for (let j = 0; j < count; j++) {
      const bx = x + (random() + random() - 1) * spread;
      const by = y + range(-3, 5);
      paint(g, { x: bx, y: by, height: height * range(.4, 1.1), lean: lean + range(-6, 6), width: range(.5, 1.15),
        bend: range(-3, 3), pigment: palette[j % 4], light: lighting.groundLight(bx, by), edge: j % 7 === 0 });
    }
  }
  for (let i = 0; i < 100; i++) {
    const x = 8 + i * 7.25 + range(-5, 5), y = groundEdge(x) + range(1, 13);
    const spread = range(10, 22), height = range(12, 30), lean = range(-height * .4, height * .4);
    const moving = [];
    for (let j = 0; j < 43; j++) {
      const offset = (random() + random() - 1) * spread;
      const bx = x + offset, by = y + range(-2, 6);
      const blade = { x: bx, y: by, height: height * range(.35, 1.1) * (1 - Math.abs(offset) / spread * .35),
        lean: lean + offset * .3 + range(-5, 5), bend: range(-4, 4), width: range(.65, 1.5),
        pigment: palette[(j + i) % 4], light: lighting.groundLight(bx, by), edge: j % 6 === 0 };
      if (i % 2 === 0 && j % 17 === 0) moving.push(blade);
      else paint(g, blade);
    }
    if (moving.length) tufts.push({ x, y, blades: moving });
  }
  return { surface, y: top, tufts, paint };
}
