import {
  bandGradient,
  clamp,
  fillDisc,
  HAZE_RAMP,
  hash,
  type Level,
  lightSpark,
  line,
  NIGHT,
  ramp,
  scanFill,
  tint,
  WARM,
} from "./pixelCore";
import { capAt, type RoofGeometry } from "./roofGeometry";

// Планы 0–2: небо, ковёр дальнего города, ближние чёрные массы. Камера смотрит СВЕРХУ
// ВНИЗ, поэтому города здесь не «силуэт на горизонте», а именно ковёр: сплошное поле
// мелких кварталов с плотной россыпью однопиксельных окон.
//
// Два правила, из которых всё вытекает:
//   1. Дальнее — СВЕТЛЕЕ и бледнее, к нему подмешана городская засветка; ближнее почти
//      чёрное. Только так глубина читается в 16 цветах.
//   2. Плотность живёт в ГОРОДЕ, а не на крыше. Ковёр окон — основной источник детали
//      всего кадра; на крыше деталей нарочно мало, чтобы они не спорили с котом.

const SKY_STOPS = [0, 0.32, 0.58, 0.8] as const;

/** Проход 1 · небо полосами. Ни одного плавного градиента — только полосы и швы. */
export function drawSky(context: CanvasRenderingContext2D, geometry: RoofGeometry) {
  const { width, skyBottom } = geometry;
  bandGradient(
    context,
    0,
    0,
    width,
    skyBottom,
    SKY_STOPS.map((at, index) => ({ at, color: ramp(NIGHT, index as Level) })),
    Math.max(4, Math.round(skyBottom / 12)),
  );
  // Засветка от города: последние строки неба уходят в тёплую лиловую дымку, и по
  // самому шву лежит полоса дизеринга — единственное место в кадре, где он уместен.
  const wash = clamp(Math.round(skyBottom * 0.3), 8, 46);
  const haze = tint(NIGHT, 3, WARM, 0.16);
  for (let step = 0; step < wash; step += 1) {
    const y = skyBottom - wash + step;
    scanFill(context, 0, y, width, 1, ramp(NIGHT, 3), haze, (step + 1) / (wash + 1));
  }
}

/** Луна: маленький диск с ступенчатым венчиком. Ночью вокруг неё почти нет свечения. */
export function drawMoon(context: CanvasRenderingContext2D, geometry: RoofGeometry) {
  const { x, y, radius } = geometry.moon;
  // Один шаг венчика, не два: две концентрические ступени читались мишенью.
  fillDisc(context, x, y, Math.round(radius * 1.7), Math.round(radius * 1.7), tint(NIGHT, 3, HAZE_RAMP, 0.22));
  fillDisc(context, x, y, radius, radius, ramp(HAZE_RAMP, 2));
  fillDisc(context, x - 1, y + 1, radius - 1, radius - 1, ramp(HAZE_RAMP, 3));
  // Два кратера на ступень темнее: иначе луна — плоский кружок.
  fillDisc(context, x - 2, y - 1, 2, 2, ramp(HAZE_RAMP, 2));
  fillDisc(context, x + 2, y + 2, 1, 1, ramp(HAZE_RAMP, 2));
}

/** Звёзды созвездиями: одиночных пикселей в пиксель-арте не бывает. */
export function drawStars(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, skyBottom } = geometry;
  const limit = Math.round(skyBottom * 0.86);
  const groups = Math.max(12, Math.round((width * limit) / 1900));
  for (let group = 0; group < groups; group += 1) {
    const baseX = Math.round(hash(seed, group, 3) * (width - 16)) + 8;
    const baseY = Math.round(hash(seed, group, 7) * Math.max(8, limit - 10)) + 3;
    const members = 1 + Math.floor(hash(seed, group, 11) * 4);
    for (let star = 0; star < members; star += 1) {
      const x = baseX + Math.round(hash(seed + group, star, 13) * 13) - 6;
      const y = baseY + Math.round(hash(seed + group, star, 17) * 10) - 4;
      // Яркость падает к горизонту: у города звёзды тонут в засветке.
      const near = y / Math.max(1, limit);
      const level: Level = near > 0.78 ? 1 : star === 0 || near < 0.4 ? 3 : 2;
      context.fillStyle = ramp(HAZE_RAMP, level);
      context.fillRect(x, y, 1, 1);
    }
  }
}

/** Перистые полосы: тонкие рваные штрихи, подсвеченные снизу засветкой города. */
export function drawClouds(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, skyBottom } = geometry;
  const banks = Math.max(2, Math.round(skyBottom / 46));
  for (let bank = 0; bank < banks; bank += 1) {
    const y = Math.round(skyBottom * (0.3 + hash(seed, bank, 5) * 0.52));
    const startX = Math.round(hash(seed, bank, 9) * width) - Math.round(width * 0.2);
    const span = Math.round(width * (0.26 + hash(seed, bank, 13) * 0.3));
    const body = tint(NIGHT, 3, HAZE_RAMP, 0.16);
    const lit = tint(NIGHT, 3, WARM, 0.22);
    let x = startX;
    while (x < startX + span) {
      const dash = 3 + Math.floor(hash(seed + bank, x, 19) * 9);
      const lift = Math.round(Math.sin(((x - startX) / Math.max(1, span)) * Math.PI) * 3);
      if (hash(seed + bank, x, 23) > 0.28) {
        context.fillStyle = body;
        context.fillRect(x, y - lift, dash, 2);
        context.fillStyle = lit;
        context.fillRect(x, y - lift + 1, dash, 1);
      }
      x += dash + 1 + Math.floor(hash(seed + bank, x, 29) * 3);
    }
  }
}

/**
 * Окна. Три правила, без которых они читаются рассыпанными кирпичами, а не городом:
 * окно всегда ОДИН пиксель на любой дистанции, большинство горит тускло (ярких единицы),
 * и горят они вертикальными пробегами — так выглядят лестничные клетки.
 */
function litWindows(
  context: CanvasRenderingContext2D,
  x: number,
  top: number,
  blockWidth: number,
  blockHeight: number,
  pitch: number,
  density: number,
  seed: number,
  bright: Level,
) {
  const columns = Math.floor((blockWidth - 1) / pitch);
  const rows = Math.floor((blockHeight - 2) / pitch);
  for (let column = 0; column < columns; column += 1) {
    const stair = hash(seed, column, 21) < 0.16;
    const chance = stair ? density * 3 : density;
    for (let row = 0; row < rows; row += 1) {
      if (hash(seed, column * 31 + row, 27) > chance) continue;
      const roll = hash(seed, column + row * 7, 33);
      context.fillStyle =
        roll > 0.95
          ? ramp(HAZE_RAMP, 1)
          : roll > 0.84
            ? ramp(WARM, bright)
            : roll > 0.44
              ? ramp(WARM, clamp(bright - 1, 0, 3) as Level)
              : ramp(WARM, clamp(bright - 2, 0, 3) as Level);
      context.fillRect(x + 1 + column * pitch, top + 2 + row * pitch, 1, 1);
    }
  }
}

/** Мелочь на дальних крышах: бак, мачта, надстройка. Силуэтом, без внутренних деталей. */
function roofClutter(
  context: CanvasRenderingContext2D,
  x: number,
  top: number,
  blockWidth: number,
  color: string,
  seed: number,
) {
  const roll = hash(seed, x, 37);
  if (roll < 0.3) {
    // Бак на ножках.
    const tankX = x + Math.round(blockWidth * 0.5);
    context.fillStyle = color;
    context.fillRect(tankX - 3, top - 5, 7, 4);
    context.fillRect(tankX - 2, top - 1, 1, 2);
    context.fillRect(tankX + 2, top - 1, 1, 2);
  } else if (roll < 0.52) {
    // Мачта с реями.
    const mastX = x + Math.round(blockWidth * (0.3 + hash(seed, x, 41) * 0.4));
    const mastHeight = 5 + Math.floor(hash(seed, x, 43) * 9);
    context.fillStyle = color;
    context.fillRect(mastX, top - mastHeight, 1, mastHeight);
    context.fillRect(mastX - 2, top - mastHeight + 2, 5, 1);
  } else if (roll < 0.7) {
    // Надстройка выхода на крышу.
    const hutX = x + Math.round(blockWidth * 0.24);
    context.fillStyle = color;
    context.fillRect(hutX, top - 4, Math.max(4, Math.round(blockWidth * 0.4)), 4);
  }
}

/**
 * Проход 2 · ковёр дальнего города. Четыре полосы глубины: верхняя почти растворена в
 * засветке, нижняя уже читается кварталами. Здесь и живёт ВСЯ плотность кадра —
 * несколько тысяч однопиксельных окон.
 */
export function drawCityCarpet(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, skyBottom, carpetBottom } = geometry;
  const depth = Math.max(8, carpetBottom - skyBottom);

  // Дальнее СВЕТЛЕЕ ближнего — иначе ковёр не читается вовсе. Верхняя полоса почти
  // растворена в засветке и заметно светлее самого неба, нижняя уже уходит в ночь.
  const rows = [
    { at: 0, body: tint(HAZE_RAMP, 1, WARM, 0.2), cap: tint(HAZE_RAMP, 2, WARM, 0.16), low: 4, high: 11, step: 7, pitch: 2, density: 0.34, bright: 2 as Level },
    { at: 0.2, body: tint(HAZE_RAMP, 0, WARM, 0.2), cap: tint(HAZE_RAMP, 1, WARM, 0.16), low: 6, high: 16, step: 10, pitch: 2, density: 0.3, bright: 2 as Level },
    { at: 0.44, body: tint(NIGHT, 3, WARM, 0.18), cap: tint(HAZE_RAMP, 0, WARM, 0.14), low: 9, high: 22, step: 14, pitch: 3, density: 0.26, bright: 3 as Level },
    { at: 0.68, body: tint(NIGHT, 2, WARM, 0.1), cap: tint(NIGHT, 3, WARM, 0.14), low: 12, high: 30, step: 19, pitch: 3, density: 0.2, bright: 3 as Level },
  ];

  for (const [rowIndex, row] of rows.entries()) {
    const baseY = skyBottom + Math.round(depth * row.at) + Math.round(depth * 0.28);
    const body = row.body;
    const cap = row.cap;
    let x = -6;
    let index = 0;
    while (x < width) {
      const blockWidth = row.step + Math.floor(hash(seed + rowIndex, index, 3) * row.step);
      const blockHeight = row.low + Math.floor(hash(seed + rowIndex, index, 7) * (row.high - row.low));
      const top = baseY - blockHeight;
      context.fillStyle = body;
      context.fillRect(x, top, blockWidth, baseY - top + Math.round(depth * 0.3));
      context.fillStyle = cap;
      context.fillRect(x, top, blockWidth, 1);
      if (rowIndex >= 2) roofClutter(context, x, top, blockWidth, body, seed + rowIndex * 13 + index);
      litWindows(context, x, top, blockWidth, blockHeight, row.pitch, row.density, seed + index * 17 + rowIndex * 101, row.bright);
      x += blockWidth + (hash(seed + rowIndex, index, 19) > 0.7 ? 2 : 0);
      index += 1;
    }
  }
}

/**
 * Проход 3 · ближние массы. Почти чёрные башни в левой половине, перекрывающие ковёр.
 * Перекрытие даёт глубину надёжнее любого блюра, а чёрное рядом со светлым — контраст,
 * из которого композиция и читается.
 */
export function drawNearBlocks(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, height, carpetBottom } = geometry;
  const body = ramp(NIGHT, 0);
  const edge = tint(NIGHT, 2, HAZE_RAMP, 0.2);
  const blocks = Math.max(4, Math.round(width / 78));

  for (let index = 0; index < blocks; index += 1) {
    const blockWidth = Math.round(width * (0.07 + hash(seed, index, 3) * 0.07));
    const x = Math.round((index / blocks) * width * 0.66) - 12 + Math.round(hash(seed, index, 5) * 20);
    const top = carpetBottom - Math.round((height - carpetBottom) * (0.1 + hash(seed, index, 7) * 0.7));
    const bottom = Math.min(height, capAt(geometry, x + blockWidth / 2) + 30);
    context.fillStyle = body;
    context.fillRect(x, top, blockWidth, bottom - top);
    // Крышка ловит лунный воздух, правая грань — саму луну. Без этих двух линий масса
    // читается плоской чёрной дырой, а не домом.
    context.fillStyle = edge;
    context.fillRect(x, top, blockWidth, 1);
    context.fillStyle = tint(NIGHT, 1, HAZE_RAMP, 0.16);
    context.fillRect(x + blockWidth - 1, top + 1, 1, bottom - top - 1);
    context.fillStyle = body;
    context.fillRect(x + 2, top - 3, 3, 3);
    context.fillRect(x + blockWidth - 6, top - 3, 3, 3);
    // Пилоны фасада: вертикали на одну ступень темнее, чтобы масса не была пустой.
    for (let pier = 1; pier * 7 < blockWidth - 2; pier += 1) {
      context.fillStyle = ramp(NIGHT, 1);
      context.fillRect(x + pier * 7, top + 2, 1, bottom - top - 3);
    }
    roofClutter(context, x, top, blockWidth, body, seed + index * 29);
    litWindows(context, x, top, blockWidth, bottom - top, 4, 0.17, seed + index * 37, 3);
    // Пожарная лестница зигзагом: самая узнаваемая деталь ближнего дома.
    if (hash(seed, index, 11) > 0.55) {
      const ladderX = x + Math.round(blockWidth * 0.68);
      for (let flight = 0; flight * 9 < bottom - top - 8; flight += 1) {
        const y = top + 6 + flight * 9;
        context.fillStyle = edge;
        context.fillRect(ladderX - 4, y, 9, 1);
        line(context, ladderX - 4, y, ladderX + 4, y + 8, edge);
      }
    }
  }
}

/** Позиции мачт с маяками: статика и мигание должны совпасть, поэтому считаются раз. */
export function beaconAnchors(geometry: RoofGeometry, seed: number) {
  const { width, skyBottom, carpetBottom } = geometry;
  return [0, 1, 2].map((index) => {
    const x = Math.round(width * (0.16 + index * 0.3)) + Math.round(hash(seed, index, 41) * 14);
    const top = skyBottom + Math.round((carpetBottom - skyBottom) * (0.1 + hash(seed, index, 43) * 0.3));
    return { x, top };
  });
}

/** Высокие мачты и кран: вертикали, разрывающие ровный ковёр кварталов. */
export function drawSkylineMasts(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { skyBottom, carpetBottom } = geometry;
  const color = tint(NIGHT, 2, WARM, 0.08);
  for (const mast of beaconAnchors(geometry, seed)) {
    const base = carpetBottom - Math.round((carpetBottom - skyBottom) * 0.1);
    context.fillStyle = color;
    context.fillRect(mast.x, mast.top, 1, base - mast.top);
    for (let arm = 0; arm < 3; arm += 1) {
      const y = mast.top + 4 + arm * Math.round((base - mast.top) / 4);
      const span = 2 + arm;
      line(context, mast.x - span, y, mast.x + span, y, color);
    }
  }
  // Строительный кран: стрела и противовес, силуэтом в один пиксель.
  const craneX = Math.round(geometry.width * 0.52);
  const craneTop = skyBottom + Math.round((carpetBottom - skyBottom) * 0.06);
  context.fillStyle = color;
  context.fillRect(craneX, craneTop, 1, Math.round((carpetBottom - craneTop) * 0.7));
  line(context, craneX - 22, craneTop + 4, craneX + 9, craneTop + 4, color);
  context.fillRect(craneX - 22, craneTop + 4, 2, 4);
  context.fillRect(craneX + 8, craneTop + 2, 2, 3);
}

/** Эстакада: цепочка тёплых точек, уходящая в перспективу. Ночной город — это движение. */
export function drawViaduct(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, carpetBottom, height } = geometry;
  const startY = carpetBottom + Math.round((height - carpetBottom) * 0.1);
  const endY = carpetBottom - Math.round((carpetBottom - geometry.skyBottom) * 0.12);
  const deck = tint(NIGHT, 1, HAZE_RAMP, 0.1);
  const columns = 26;
  for (let step = 0; step < columns; step += 1) {
    const t = step / (columns - 1);
    const x = Math.round(width * (0.02 + t * 0.5));
    const y = Math.round(startY + (endY - startY) * t ** 0.7);
    const thickness = Math.max(1, Math.round(4 - t * 3));
    context.fillStyle = deck;
    context.fillRect(x, y, Math.ceil((width * 0.5) / columns) + 1, thickness);
    // Опоры реже, чем полотно: каждая четвёртая.
    if (step % 4 === 0) context.fillRect(x + 1, y + thickness, 1, Math.round(12 - t * 9));
    if (hash(seed, step, 53) > 0.45) {
      context.fillStyle = ramp(WARM, hash(seed, step, 59) > 0.7 ? 3 : 2);
      context.fillRect(x + 2, y - 1, 1, 1);
    }
  }
}

/** Динамика города: маяки на мачтах и самолёт, ползущий над ковром. */
export function drawCityPulse(
  context: CanvasRenderingContext2D,
  geometry: RoofGeometry,
  frame: number,
  seed: number,
) {
  const { width, skyBottom } = geometry;
  const beat = Math.floor(frame / 6) % 4;
  for (const [index, mast] of beaconAnchors(geometry, seed).entries()) {
    if ((beat + index) % 4 !== 0) continue;
    lightSpark(context, mast.x, mast.top - 1, 2, WARM, 3);
  }
  // Самолёт: два пикселя корпуса и мигающий огонь. Проходит кадр за один цикл.
  const span = width + 60;
  const planeX = ((Math.floor(frame / 3) * 2) % span) - 30;
  const planeY = Math.round(skyBottom * 0.3);
  context.fillStyle = ramp(HAZE_RAMP, 1);
  context.fillRect(planeX, planeY, 2, 1);
  if (frame % 8 < 2) {
    context.fillStyle = ramp(WARM, 3);
    context.fillRect(planeX + 2, planeY, 1, 1);
  }
}
