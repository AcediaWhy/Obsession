import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import { bakeArt, type BakedArt, SHOP_SOURCES, TRANSPARENT } from "./artBake";
import { extractSprites, type Sprite } from "./artSprites";

// Сцена собирается из АВТОРСКИХ спрайтов: стена с полкой, кот, пряди плюща, горшки.
// Ни один пиксель здесь не нарисован мной — код только расставляет и оживляет.
//
// Замеры листа стены (после восстановления сетки, координаты в кадре 256×256):
//   содержимое  x 35…179, y 6…246
//   полка       верх на y 160, пробег x 40…180
// Полка — единственная опорная линия сцены: на ней сидит кот и стоят горшки.

const WALL_CONTENT = { x: 35, y: 6, w: 145, h: 241 } as const;
const SHELF_TOP = 160;

export type ShopArt = {
  art: BakedArt;
  /** Палитра в виде компонент — нужна для палитровой анимации без разбора строк. */
  rgb: [number, number, number][];
  wall: Sprite;
  cat: Sprite;
  ivy: Sprite[];
  pots: Sprite[];
  /** Пиксели глаз кота и цвет шерсти: моргание гасит именно их, а не «всё светлое». */
  catEyes: number[];
  catFur: number;
};

let cache: ShopArt | null = null;
let pending: Promise<ShopArt> | null = null;

function channels(hex: string): [number, number, number] {
  return [
    Number.parseInt(hex.slice(1, 3), 16),
    Number.parseInt(hex.slice(3, 5), 16),
    Number.parseInt(hex.slice(5, 7), 16),
  ];
}

/**
 * Глаза кота находятся сами: это два самых крупных СВЯЗНЫХ пятна светлых пикселей внутри
 * почти чёрного силуэта. Искать «самый светлый индекс» было нельзя — под него попадали и
 * усы, и они гасли вместе с глазами.
 */
function findCatEyes(cat: Sprite, rgb: readonly [number, number, number][]) {
  const bright = new Uint8Array(cat.width * cat.height);
  for (let i = 0; i < cat.indices.length; i += 1) {
    const value = cat.indices[i];
    if (value === TRANSPARENT) continue;
    const [r, g, b] = rgb[value];
    if (r * 0.299 + g * 0.587 + b * 0.114 > 150) bright[i] = 1;
  }
  const seen = new Uint8Array(bright.length);
  const clusters: number[][] = [];
  for (let start = 0; start < bright.length; start += 1) {
    if (seen[start] || !bright[start]) continue;
    const stack = [start];
    seen[start] = 1;
    const cells: number[] = [];
    while (stack.length > 0) {
      const index = stack.pop() as number;
      const x = index % cat.width;
      const y = (index - x) / cat.width;
      cells.push(index);
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const nx = x + dx;
        const ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= cat.width || ny >= cat.height) continue;
        const next = ny * cat.width + nx;
        if (seen[next] || !bright[next]) continue;
        seen[next] = 1;
        stack.push(next);
      }
    }
    clusters.push(cells);
  }
  clusters.sort((first, second) => second.length - first.length);
  return clusters.slice(0, 2).flat();
}

function darkestIndex(sprite: Sprite, rgb: readonly [number, number, number][]) {
  let index = 0;
  let luma = Number.POSITIVE_INFINITY;
  for (const value of sprite.indices) {
    if (value === TRANSPARENT) continue;
    const [r, g, b] = rgb[value];
    const current = r * 0.299 + g * 0.587 + b * 0.114;
    if (current < luma) {
      luma = current;
      index = value;
    }
  }
  return index;
}

/** Печём один раз за сессию: разбор четырёх PNG по 1024² стоит около сотни миллисекунд. */
export function loadShopArt(): Promise<ShopArt> {
  if (cache) return Promise.resolve(cache);
  if (pending) return pending;
  pending = (async () => {
    const art = await bakeArt(SHOP_SOURCES, 48);
    const rgb = art.palette.map(channels);
    const wall = extractSprites(art, "wall", 4000)[0];
    const cat = extractSprites(art, "cat", 400)[0];
    const ivy = extractSprites(art, "ivy", 90);
    const pots = extractSprites(art, "pots", 400);
    cache = { art, rgb, wall, cat, ivy, pots, catEyes: findCatEyes(cat, rgb), catFur: darkestIndex(cat, rgb) };
    return cache;
  })();
  return pending;
}

/** Раскладка сцены в координатах буфера. Считается от полки: она держит всю композицию. */
export type ShopLayout = {
  wallX: number;
  wallY: number;
  shelfY: number;
  catX: number;
  catY: number;
};

export function shopLayout(art: ShopArt, width: number, height: number): ShopLayout {
  // Стена прижата к низу и чуть отступает от левого края: в боевом окне там рельса меню,
  // и композиция должна начинаться правее неё.
  const wallX = Math.max(6, Math.round(width * 0.05)) - WALL_CONTENT.x;
  const wallY = height - (WALL_CONTENT.y + WALL_CONTENT.h);
  const shelfY = wallY + SHELF_TOP;
  return {
    wallX,
    wallY,
    shelfY,
    catX: wallX + WALL_CONTENT.x + 34,
    catY: shelfY + 2 - art.cat.height,
  };
}

/** Отрисовка спрайта в буфер индексов с необязательным сдвигом строк (качание, маятник). */
function blit(
  target: Uint8Array,
  size: { width: number; height: number },
  sprite: Sprite,
  originX: number,
  originY: number,
  rowShift?: (row: number) => number,
) {
  for (let y = 0; y < sprite.height; y += 1) {
    const ty = originY + y;
    if (ty < 0 || ty >= size.height) continue;
    const shift = rowShift ? rowShift(y) : 0;
    for (let x = 0; x < sprite.width; x += 1) {
      const value = sprite.indices[y * sprite.width + x];
      if (value === TRANSPARENT) continue;
      const tx = originX + x + shift;
      if (tx < 0 || tx >= size.width) continue;
      target[ty * size.width + tx] = value;
    }
  }
}

/**
 * Качание листвы: сдвиг ЦЕЛЫХ строк спрайта на целое число пикселей. Стиль сохраняется сам
 * собой, потому что двигаются авторские пиксели, а новых не появляется. Амплитуда растёт к
 * свободному концу пряди — у крепления она нулевая.
 */
function sway(height: number, frame: number, phase: number, amplitude: number) {
  return (row: number) => {
    const grip = row / Math.max(1, height - 1);
    return Math.round(Math.sin(frame * 0.12 + phase) * amplitude * grip * grip);
  };
}

/** Маятник кашпо: ось у самого верха, поэтому сдвиг линейный по высоте. */
function pendulum(height: number, frame: number, phase: number, amplitude: number) {
  return (row: number) => Math.round(Math.sin(frame * 0.09 + phase) * amplitude * (row / Math.max(1, height - 1)));
}

/**
 * Земля под растениями. Своих пикселей не рисую: беру нижнюю полосу дощатой обшивки из
 * листа стены и повторяю её вправо со сдвигом фазы, чтобы стык досок не читался штампом.
 */
function tileGround(
  target: Uint8Array,
  size: { width: number; height: number },
  wall: Sprite,
  fromX: number,
  originY: number,
) {
  const stripTop = WALL_CONTENT.y + WALL_CONTENT.h - 15;
  const stripLeft = WALL_CONTENT.x + 6;
  const stripWidth = WALL_CONTENT.w - 14;
  for (let tile = 0; fromX + tile * stripWidth < size.width; tile += 1) {
    const shift = tile % 2 === 0 ? 0 : 7;
    for (let y = 0; y < 15; y += 1) {
      const ty = originY + y;
      if (ty < 0 || ty >= size.height) continue;
      for (let x = 0; x < stripWidth; x += 1) {
        const source = wall.indices[(stripTop + y) * wall.width + stripLeft + ((x + shift) % stripWidth)];
        if (source === TRANSPARENT) continue;
        const tx = fromX + tile * stripWidth + x;
        if (tx < 0 || tx >= size.width) continue;
        target[ty * size.width + tx] = source;
      }
    }
  }
}

export type ShopFrame = {
  /** Индексы палитры на кадр. */
  indices: Uint8Array;
  width: number;
  height: number;
};

const scratch = { buffer: new Uint8Array(0), width: 0, height: 0 };

/**
 * Кадр сцены. Листы автора — не масштабный набор, а отдельные портреты предметов: герань
 * ростом с кота, папоротник крупнее. Поэтому композиция читается не как «крыльцо с
 * мелочью», а как ФРОНТ ЦВЕТОЧНОЙ ЛАВКИ: крупные растения стоят на земле, кот сидит на
 * полке, плющ свисает сверху. В таком прочтении их размеры сразу становятся верными.
 *
 * Порядок наложения: паспарту → дальняя прядь → стена с полкой → напольные растения →
 * кот → кашпо и передние пряди. Передний плющ и даёт глубину.
 */
export function composeShopFrame(
  art: ShopArt,
  width: number,
  height: number,
  phase: ObsessionVisualPhase,
  frame: number,
): ShopFrame {
  if (scratch.width !== width || scratch.height !== height) {
    scratch.buffer = new Uint8Array(width * height);
    scratch.width = width;
    scratch.height = height;
  }
  const target = scratch.buffer;
  const size = { width, height };
  // Паспарту автора служит небом: это его же цвет, самый частый по рамке листа стены.
  target.fill(art.wall.indices[0]);

  const layout = shopLayout(art, width, height);
  const { wallX, wallY, catX, catY } = layout;
  const wallRight = wallX + WALL_CONTENT.x + WALL_CONTENT.w;

  // Дальняя прядь уходит за стену у самого края кадра.
  const far = art.ivy[2];
  if (far) blit(target, size, far, wallX + 4, wallY - 34, sway(far.height, frame, 1.7, 2));

  blit(target, size, art.wall, wallX, wallY);
  // Земля вправо от стены: та же обшивка, иначе растения стоят на пустом паспарту.
  tileGround(target, size, art.wall, wallRight - 8, height - 15);

  // Напольные растения выстроены вправо по земле — ими и заполняется широкий кадр.
  const floor: { sprite: Sprite | undefined; x: number; phase: number }[] = [
    { sprite: art.pots[0], x: wallRight + 4, phase: 0.4 },
    { sprite: art.pots[1], x: wallRight + 108, phase: 1.9 },
    { sprite: art.pots[2], x: wallRight + 186, phase: 0 },
    { sprite: art.pots[4], x: wallRight + 236, phase: 0 },
    { sprite: art.pots[6], x: wallRight + 274, phase: 0 },
  ];
  for (const item of floor) {
    if (!item.sprite || item.x > width) continue;
    const nod = item.phase > 0
      ? sway(item.sprite.height, frame, item.phase, 1.4)
      : undefined;
    // Листва качается, а горшки стоят: сдвиг применяем только к верхней половине спрайта.
    blit(target, size, item.sprite, item.x, height - item.sprite.height,
      nod ? (row) => (row < item.sprite!.height * 0.55 ? nod(item.sprite!.height - row) : 0) : undefined);
  }

  blit(target, size, art.cat, catX, catY);
  drawCatLife(target, size, art, layout, phase, frame);

  // Кашпо на шнурах и передние пряди: они висят перед всем и задают глубину.
  const planters = [art.pots[3], art.pots[5]];
  for (const [index, planter] of planters.entries()) {
    if (!planter) continue;
    const x = wallRight + 52 + index * 168;
    if (x > width) continue;
    blit(target, size, planter, x, -6, pendulum(planter.height, frame, index * 1.3, 2));
  }
  const near = art.ivy[1];
  const tip = art.ivy[4];
  const spare = art.ivy[3];
  if (near) blit(target, size, near, wallX + WALL_CONTENT.x + 26, wallY - 26, sway(near.height, frame, 0, 3));
  if (tip) blit(target, size, tip, wallRight + 74, -8, sway(tip.height, frame, 2.4, 3));
  if (spare) blit(target, size, spare, wallRight + 210, -6, sway(spare.height, frame, 3.1, 3));

  return { indices: target, width, height };
}

/**
 * Оживление кота. Пиксели глаз найдены при загрузке как два крупнейших светлых пятна внутри
 * силуэта, поэтому моргание гасит именно их, а не усы заодно. Прищур в `focused` гасит
 * каждую третью строку белка — глаз сужается, а не закрывается.
 */
function drawCatLife(
  target: Uint8Array,
  size: { width: number; height: number },
  art: ShopArt,
  layout: ShopLayout,
  phase: ObsessionVisualPhase,
  frame: number,
) {
  const { catX, catY } = layout;
  const cat = art.cat;
  const blink = phase === "fault" ? frame % 9 < 3 : frame % 41 < 2;
  const squint = phase === "focused";
  if (!blink && !squint) return;

  for (const cell of art.catEyes) {
    const x = cell % cat.width;
    const y = (cell - x) / cat.width;
    if (squint && !blink && y % 3 !== 2) continue;
    const tx = catX + x;
    const ty = catY + y;
    if (tx < 0 || ty < 0 || tx >= size.width || ty >= size.height) continue;
    target[ty * size.width + tx] = art.catFur;
  }
}

/** Разворот индексов в пиксели канваса. Палитру передаём отдельно — её меняет анимация. */
export function paintShopFrame(
  context: CanvasRenderingContext2D,
  shopFrame: ShopFrame,
  palette: readonly [number, number, number][],
) {
  const image = context.createImageData(shopFrame.width, shopFrame.height);
  const data = image.data;
  for (let i = 0; i < shopFrame.indices.length; i += 1) {
    const colour = palette[shopFrame.indices[i]];
    const at = i * 4;
    data[at] = colour[0];
    data[at + 1] = colour[1];
    data[at + 2] = colour[2];
    data[at + 3] = 255;
  }
  context.putImageData(image, 0, 0);
}

/**
 * Палитровая анимация: меняем ЗАПИСИ палитры, а не пиксели. Один проход по сорока восьми
 * цветам вместо ста тысяч точек — так в восьмибитных играх и делали свет.
 *   idle      медленное дыхание тёплого света
 *   focused   тёплые ступени поднимаются
 *   fault     кадр уезжает в холод и обесцвечивается
 */
export function shopPalette(
  base: readonly [number, number, number][],
  phase: ObsessionVisualPhase,
  frame: number,
): [number, number, number][] {
  const breath = 0.5 + Math.sin(frame * 0.05) * 0.5;
  const warm = phase === "focused" ? 0.16 : phase === "engaging" ? 0.08 : breath * 0.05;
  const cool = phase === "fault" ? 0.4 : phase === "scanning" ? 0.12 : 0;
  return base.map(([r, g, b]) => {
    const grey = r * 0.299 + g * 0.587 + b * 0.114;
    const wr = r + (255 - r) * warm;
    const wg = g + (245 - g) * warm * 0.7;
    const wb = b * (1 - warm * 0.25);
    return [
      Math.round(wr + (grey * 0.82 - wr) * cool),
      Math.round(wg + (grey * 0.88 - wg) * cool),
      Math.round(wb + (grey * 1.12 - wb) * cool),
    ];
  });
}
