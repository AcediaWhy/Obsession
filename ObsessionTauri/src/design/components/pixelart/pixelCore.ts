// Ядро процедурного пиксель-арта лабы. Собрано по методике Slynyrd: цвет решает
// глубину, текстура делается КЛАСТЕРАМИ (а не одиночными пикселями), переходы —
// упорядоченным дизерингом, деталь падает с дистанцией.
//
// Три правила, которые здесь зашиты жёстко, потому что все прошлые попытки
// разваливались именно на них:
//   1. Дальнее — светлее и бледнее, ближнее — темнее и контрастнее. Не наоборот.
//   2. Никаких «сирот»: минимальная единица текстуры — кластер из 2+ пикселей.
//   3. Текстура неравномерна. Плотные участки обязаны соседствовать с пустыми.

export type Level = 0 | 1 | 2 | 3;
export type Material = "night" | "haze" | "warm" | "solid";
export type PlaneIndex = 0 | 1 | 2 | 3;
export type PixelPoint = { x: number; y: number };
export type Ramp = readonly [string, string, string, string];

// Глухая ночь над городом: 4 рампы × 4 ступени = ровно 16 цветов.
//   night — воздух, от зенита к горизонту;
//   haze  — БЛЕДНАЯ холодная рампа: дальний город, луна, кромки от неба. Только она
//           и даёт глубину: далёкое обязано быть светлее ближнего;
//   warm  — ВЕСЬ тёплый свет кадра: окна, лампы, молоко. Больше нигде тепла нет;
//   solid — всё осязаемое: бетон, металл, ящики. Предметы отделяются друг от друга
//           ступенью и контуром, а не отдельной рампой.
export const RAMP: Record<Material, Ramp> = {
  night: ["#0a0c16", "#111527", "#1a2039", "#262e4d"],
  haze: ["#333a5e", "#4b5480", "#6e79a3", "#a9b2cf"],
  warm: ["#6d3f2b", "#a86530", "#e29b3e", "#ffdd96"],
  solid: ["#131419", "#1f212a", "#2f3341", "#464b5e"],
};

export const MATERIALS: readonly Material[] = ["night", "haze", "warm", "solid"];
export const NIGHT: Material = "night";
export const HAZE_RAMP: Material = "haze";
export const WARM: Material = "warm";
export const SOLID: Material = "solid";

/** Воздух горизонта: к нему притягиваются дальние планы. */
const HAZE = RAMP.haze[1];
const INK = "#07080f";

// Ремап плана: дальнее подмешивается к дымке, ближнее — к чернилам. Это и есть
// атмосферная перспектива, единственный честный способ дать глубину в 16 цветах.
const PLANE_MIX: Record<PlaneIndex, { haze: number; ink: number }> = {
  0: { haze: 0.22, ink: 0 },
  1: { haze: 0.16, ink: 0 },
  2: { haze: 0.05, ink: 0.05 },
  3: { haze: 0, ink: 0.06 },
};

export function clamp(value: number, low: number, high: number) {
  return value < low ? low : value > high ? high : value;
}

/** Детерминированный шум: одна и та же крыша при каждом ресайзе. */
export function hash(a: number, b: number, c: number) {
  let value = Math.imul(a ^ 0x9e3779b9, 0x85ebca6b);
  value ^= Math.imul(b + 0x165667b1, 0xc2b2ae35);
  value ^= Math.imul(c + 0x27d4eb2f, 0x165667b1);
  value ^= value >>> 15;
  return ((value >>> 0) % 100000) / 100000;
}

function channels(hex: string) {
  return [
    Number.parseInt(hex.slice(1, 3), 16),
    Number.parseInt(hex.slice(3, 5), 16),
    Number.parseInt(hex.slice(5, 7), 16),
  ] as const;
}

function toHex(r: number, g: number, b: number) {
  const part = (value: number) => clamp(Math.round(value), 0, 255).toString(16).padStart(2, "0");
  return `#${part(r)}${part(g)}${part(b)}`;
}

function mixHex(from: string, to: string, amount: number) {
  if (amount <= 0) return from;
  const [r1, g1, b1] = channels(from);
  const [r2, g2, b2] = channels(to);
  return toHex(r1 + (r2 - r1) * amount, g1 + (g2 - g1) * amount, b1 + (b2 - b1) * amount);
}

let activePlane: PlaneIndex = 2;
const planeCache: Record<PlaneIndex, Map<string, string>> = { 0: new Map(), 1: new Map(), 2: new Map(), 3: new Map() };

/** Цвет, пересчитанный под конкретный план глубины. */
export function plane(index: PlaneIndex, color: string) {
  const cache = planeCache[index];
  const cached = cache.get(color);
  if (cached) return cached;
  const { haze, ink } = PLANE_MIX[index];
  const value = mixHex(mixHex(color, HAZE, haze), INK, ink);
  cache.set(color, value);
  return value;
}

/** Все заливки внутри body уходят в указанный план. */
export function withPlane(index: PlaneIndex, body: () => void) {
  const previous = activePlane;
  activePlane = index;
  try {
    body();
  } finally {
    activePlane = previous;
  }
}

/** Ступень рампы в текущем плане. */
export function ramp(material: Material, level: Level) {
  return plane(activePlane, RAMP[material][level]);
}

/** Ступень рампы без ремапа — для свотчей инспектора. */
export function rawRamp(material: Material, level: Level) {
  return RAMP[material][level];
}

/** Смесь двух ступеней: для 1px-кромок, где нужен полушаг. */
export function tint(material: Material, level: Level, toward: Material, amount: number) {
  return plane(activePlane, mixHex(RAMP[material][level], RAMP[toward][level], amount));
}

// ─── Дизеринг ─────────────────────────────────────────────────────────────────
// Bayer 4×4 — единственная маска для площадей, скан-строки — для неба. Тумблер
// инспектора выключает дизеринг целиком: тогда видно жёсткие стыки ступеней.

const BAYER4 = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
];

let ditherEnabled = true;
export function setDither(enabled: boolean) {
  ditherEnabled = enabled;
}

export function bayer(x: number, y: number) {
  return (BAYER4[((y % 4) + 4) % 4][((x % 4) + 4) % 4] + 0.5) / 16;
}

/**
 * Заливка смесью двух цветов через Bayer. ratio 0 → только `from`, 1 → только `to`.
 * Пиксели пишутся пробегами: 40k вызовов fillRect на кадр канвас не любит.
 */
export function ditherFill(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  from: string,
  to: string,
  ratio: number,
) {
  if (width <= 0 || height <= 0) return;
  if (!ditherEnabled) {
    context.fillStyle = ratio >= 0.5 ? to : from;
    context.fillRect(x, y, width, height);
    return;
  }
  for (let row = 0; row < height; row += 1) {
    const py = y + row;
    let runStart = 0;
    let runOn = bayer(x, py) < ratio;
    for (let column = 0; column <= width; column += 1) {
      const on = column < width ? bayer(x + column, py) < ratio : !runOn;
      if (on === runOn) continue;
      context.fillStyle = runOn ? to : from;
      context.fillRect(x + runStart, py, column - runStart, 1);
      runStart = column;
      runOn = on;
    }
  }
}

/** Только «включённые» пиксели маски — для свечений и вуалей поверх геометрии. */
export function ditherOver(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  color: string,
  ratio: number,
) {
  if (width <= 0 || height <= 0 || ratio <= 0) return;
  if (!ditherEnabled) {
    if (ratio < 0.5) return;
    context.fillStyle = color;
    context.fillRect(x, y, width, height);
    return;
  }
  context.fillStyle = color;
  for (let row = 0; row < height; row += 1) {
    const py = y + row;
    let runStart = -1;
    for (let column = 0; column <= width; column += 1) {
      const on = column < width && bayer(x + column, py) < ratio;
      if (on && runStart < 0) runStart = column;
      else if (!on && runStart >= 0) {
        context.fillRect(x + runStart, py, column - runStart, 1);
        runStart = -1;
      }
    }
  }
}

// Скан-дизеринг: чередование целых строк. В небе он честнее Bayer — небо в рефах
// расслоено именно строками, а не шашкой.
const SCAN8 = [0, 4, 2, 6, 1, 5, 3, 7];

export function scanFill(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  from: string,
  to: string,
  ratio: number,
) {
  for (let row = 0; row < height; row += 1) {
    const py = y + row;
    const on = ditherEnabled ? (SCAN8[((py % 8) + 8) % 8] + 0.5) / 8 < ratio : ratio >= 0.5;
    context.fillStyle = on ? to : from;
    context.fillRect(x, py, width, 1);
  }
}

/**
 * Небо и земля строятся полосами — шаг, на котором по Pixelblog 62 и решается
 * глубина. Стыки соседних полос размываются скан-дизерингом, а не градиентом.
 */
export function bandGradient(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  stops: readonly { readonly at: number; readonly color: string }[],
  seam = 8,
) {
  if (height <= 0) return;
  const edges = stops.map((stop) => y + Math.round(stop.at * height));
  for (let index = 0; index < stops.length; index += 1) {
    const top = edges[index];
    const bottom = index + 1 < stops.length ? edges[index + 1] : y + height;
    if (bottom <= top) continue;
    context.fillStyle = stops[index].color;
    context.fillRect(x, top, width, bottom - top);
  }
  // Швы: полоса перехода живёт по обе стороны границы, поэтому стык не читается
  // линией. Ratio ползёт от 0 до 1 — это и есть «полоса дизеринга ≥3px».
  for (let index = 1; index < stops.length; index += 1) {
    const edge = edges[index];
    const span = Math.min(seam, Math.max(3, Math.round(height / (stops.length * 2))));
    for (let step = 0; step < span * 2; step += 1) {
      const py = edge - span + step;
      if (py < y || py >= y + height) continue;
      const ratio = (step + 1) / (span * 2 + 1);
      scanFill(context, x, py, width, 1, stops[index - 1].color, stops[index].color, ratio);
    }
  }
}

// ─── Кластеры вместо одиночных пикселей ───────────────────────────────────────
// Словарь мелких форм. Текстура собирается штамповкой этих форм с переменной
// плотностью: так она читается как поверхность, а не как шум.

export type Cluster = readonly (readonly [number, number])[];

export const CLUSTERS = {
  dash2: [[0, 0], [1, 0]],
  dash3: [[0, 0], [1, 0], [2, 0]],
  blob: [[0, 0], [1, 0], [0, 1], [1, 1]],
  cee: [[1, 0], [2, 0], [0, 1], [0, 2], [1, 3], [2, 3]],
  ess: [[1, 0], [2, 0], [1, 1], [0, 2], [1, 2], [2, 2]],
  grit: [[0, 0], [1, 0], [2, 1], [1, 1]],
  leaf: [[1, 0], [2, 0], [0, 1], [1, 1], [2, 1], [1, 2]],
} as const satisfies Record<string, Cluster>;

export type ClusterName = keyof typeof CLUSTERS;

export function stamp(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  cluster: Cluster,
  color: string,
) {
  context.fillStyle = color;
  for (const [dx, dy] of cluster) context.fillRect(x + dx, y + dy, 1, 1);
}

/**
 * Штамповка словаря по площади. `density` — доля ячеек сетки, которые получают
 * кластер; `falloff` гасит плотность по вертикали, чтобы текстура не была ровной
 * простынёй (правило контраста: занятое рядом с пустым).
 */
export function scatter(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  names: readonly ClusterName[],
  color: string,
  density: number,
  seed: number,
  cell = 5,
) {
  const columns = Math.ceil(width / cell);
  const rows = Math.ceil(height / cell);
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const roll = hash(seed, column, row);
      // Второй бросок сгущает кластеры в пятна: плотность сама становится неровной.
      const local = density * (0.45 + hash(seed + 11, column >> 1, row >> 1) * 1.35);
      if (roll > local) continue;
      const name = names[Math.floor(hash(seed + 3, column, row) * names.length) % names.length];
      stamp(
        context,
        x + column * cell + Math.floor(hash(seed + 5, column, row) * cell),
        y + row * cell + Math.floor(hash(seed + 7, column, row) * cell),
        CLUSTERS[name],
        color,
      );
    }
  }
}

// ─── Формы ────────────────────────────────────────────────────────────────────

export function fillDisc(
  context: CanvasRenderingContext2D,
  centerX: number,
  centerY: number,
  radiusX: number,
  radiusY: number,
  color: string,
) {
  context.fillStyle = color;
  const top = Math.ceil(centerY - radiusY);
  const bottom = Math.floor(centerY + radiusY);
  for (let y = top; y <= bottom; y += 1) {
    const normalized = (y - centerY) / Math.max(0.5, radiusY);
    const half = Math.floor(radiusX * Math.sqrt(Math.max(0, 1 - normalized * normalized)));
    if (half <= 0) continue;
    context.fillRect(Math.round(centerX - half), y, half * 2, 1);
  }
}

export function line(
  context: CanvasRenderingContext2D,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  color: string,
  thickness = 1,
) {
  context.fillStyle = color;
  let x = Math.round(x0);
  let y = Math.round(y0);
  const endX = Math.round(x1);
  const endY = Math.round(y1);
  const stepX = x < endX ? 1 : -1;
  const stepY = y < endY ? 1 : -1;
  const deltaX = Math.abs(endX - x);
  const deltaY = -Math.abs(endY - y);
  let error = deltaX + deltaY;
  for (;;) {
    context.fillRect(x, y, thickness, thickness);
    if (x === endX && y === endY) break;
    const doubled = error * 2;
    if (doubled >= deltaY) { error += deltaY; x += stepX; }
    if (doubled <= deltaX) { error += deltaX; y += stepY; }
  }
}

export function fillPolygon(
  context: CanvasRenderingContext2D,
  points: readonly PixelPoint[],
  color: string,
) {
  if (points.length < 3) return;
  let top = points[0].y;
  let bottom = points[0].y;
  for (const point of points) {
    if (point.y < top) top = point.y;
    if (point.y > bottom) bottom = point.y;
  }
  context.fillStyle = color;
  for (let y = Math.round(top); y <= Math.round(bottom); y += 1) {
    const crossings: number[] = [];
    for (let index = 0; index < points.length; index += 1) {
      const a = points[index];
      const b = points[(index + 1) % points.length];
      if (a.y === b.y) continue;
      const low = Math.min(a.y, b.y);
      const high = Math.max(a.y, b.y);
      if (y < low || y >= high) continue;
      crossings.push(a.x + ((y - a.y) / (b.y - a.y)) * (b.x - a.x));
    }
    crossings.sort((first, second) => first - second);
    for (let pair = 0; pair + 1 < crossings.length; pair += 2) {
      const startX = Math.round(crossings[pair]);
      const endX = Math.round(crossings[pair + 1]);
      if (endX > startX) context.fillRect(startX, y, endX - startX, 1);
    }
  }
}

/**
 * Свет СТУПЕНЯМИ, а не дизером. Дизерная шашка по большой площади читается муаром —
 * той самой «рябью», которой нет ни на одном приличном пиксель-арт-референсе. Настоящий
 * свет в пиксель-арте — две-три сплошные ступени с жёсткими кромками; шум допустим
 * только на самой внешней кромке, шириной в один пиксель.
 */
export function lightPool(
  context: CanvasRenderingContext2D,
  centerX: number,
  centerY: number,
  radiusX: number,
  radiusY: number,
  material: Material,
  brightest: Level,
  steps = 3,
) {
  const count = Math.max(1, Math.min(steps, brightest + 1));
  for (let step = 0; step < count; step += 1) {
    const t = 1 - step / count;
    const level = clamp(brightest - (count - 1 - step), 0, 3) as Level;
    const rx = Math.round(radiusX * t);
    const ry = Math.round(radiusY * t);
    if (rx < 1 || ry < 1) continue;
    if (step === 0) {
      // Внешняя кромка — единственное место, где свет вправе рассыпаться.
      const outer = Math.max(1, Math.round(Math.min(rx, ry) * 0.22));
      for (let y = -ry; y <= ry; y += 1) {
        const ny = y / ry;
        const half = Math.floor(rx * Math.sqrt(Math.max(0, 1 - ny * ny)));
        const inner = Math.floor((rx - outer) * Math.sqrt(Math.max(0, 1 - ny * ny)));
        for (let x = -half; x <= half; x += 1) {
          const edge = Math.abs(x) > inner;
          if (edge && bayer(centerX + x, centerY + y) > 0.45) continue;
          context.fillStyle = ramp(material, level);
          context.fillRect(centerX + x, centerY + y, 1, 1);
        }
      }
      continue;
    }
    fillDisc(context, centerX, centerY, rx, ry, ramp(material, level));
  }
}

/** Точечный источник: ядро в один-два пикселя и ступенчатый венчик вокруг. */
export function lightSpark(
  context: CanvasRenderingContext2D,
  centerX: number,
  centerY: number,
  radius: number,
  material: Material,
  brightest: Level,
) {
  fillDisc(context, centerX, centerY, radius, radius, ramp(material, clamp(brightest - 2, 0, 3) as Level));
  fillDisc(context, centerX, centerY, Math.max(1, radius - 2), Math.max(1, radius - 2), ramp(material, clamp(brightest - 1, 0, 3) as Level));
  context.fillStyle = ramp(material, brightest);
  context.fillRect(centerX, centerY, 1, 1);
}

/**
 * Бюджет деталей от площади на экране. Правило Slynyrd: на 48px каждый пиксель
 * осмыслен, на 1600px деталей в десятки раз больше — но не в разы больше НА
 * пиксель. Корень площади и даёт этот масштаб.
 */
export function detailBudget(area: number, per1000 = 1) {
  return Math.max(1, Math.round(Math.sqrt(area) * 0.045 * per1000));
}

/** Целочисленный дрейф по ветру: субпиксельное движение убивает пиксельность. */
export function drift(seed: number, frame: number, span: number, speedDivisor: number) {
  const offset = Math.floor(hash(seed, 0, 1) * span);
  return (offset + Math.floor(frame / speedDivisor)) % span;
}

/**
 * Свет на крыше приходит СНИЗУ-СЗАДИ — от зарева и города. Значит горизонтальные
 * грани ловят холодный воздух, а всё, что смотрит на зарево, теплеет. Одна функция
 * на весь кадр, чтобы направление нельзя было случайно перевернуть.
 */
export function skyLit(base: Level, facing: "top" | "front" | "side" | "under"): Level {
  const shift = facing === "top" ? 1 : facing === "front" ? 0 : facing === "side" ? -1 : -1;
  return clamp(base + shift, 0, 3) as Level;
}

/** Досягаемость тёплого источника: 1 в центре, 0 за границей эллипса. */
export function warmReach(
  x: number,
  y: number,
  sourceX: number,
  sourceY: number,
  radiusX: number,
  radiusY: number,
) {
  const distance = Math.hypot((x - sourceX) / radiusX, (y - sourceY) / radiusY);
  return clamp(1 - distance, 0, 1);
}

