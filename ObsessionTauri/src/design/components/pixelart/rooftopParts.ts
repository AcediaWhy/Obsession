import {
  clamp,
  fillDisc,
  fillPolygon,
  HAZE_RAMP,
  hash,
  type Level,
  line,
  NIGHT,
  ramp,
  scatter,
  skyLit,
  SOLID,
  stamp,
  tint,
  WARM,
  warmReach,
} from "./pixelCore";
import { capAt, type RoofGeometry } from "./roofGeometry";

// План 3 · крыша. Деталей здесь нарочно немного: по референсу вся плотность кадра живёт
// в городе, а на крыше — миска, тряпка, лужа и пара мелочей. Стоит завалить настил
// баками и ящиками, как они начинают спорить с котом, и композиция рассыпается.
//
// Размеры считаются в единицах роста кота (geometry.unit), поэтому предметы соотносятся
// друг с другом, а не с высотой окна.
//
// Свет ОДИН и он лунный: холодный, сверху-справа. Значит настил бледнеет к зрителю и к
// правому краю, крышка парапета ловит холодную кромку по всей диагонали, а тени тянутся
// вниз-влево и длиннее самих предметов. Тёплого на крыше две крупицы: щель люка и молоко.

/** Насколько точка открыта луне: 1 прямо под ней, 0 у противоположного края кадра. */
export function moonReach(x: number, y: number, geometry: RoofGeometry) {
  const { moon, width, height } = geometry;
  return warmReach(x, y, moon.x, moon.y, width * 1.15, height * 1.6);
}

/** Ступень грани под луной. base — собственный тон материала. */
export function litLevel(
  x: number,
  y: number,
  geometry: RoofGeometry,
  base: Level,
  facing: "top" | "front" | "side" | "under",
) {
  const shift = moonReach(x, y, geometry) > 0.42 ? 1 : 0;
  return clamp(skyLit(base, facing) + shift, 0, 3) as Level;
}

/** Тень от луны: она высоко и справа, поэтому тень падает вниз-влево и длиннее предмета. */
export function castShadow(
  context: CanvasRenderingContext2D,
  x: number,
  baseY: number,
  width: number,
  objectHeight: number,
) {
  const reach = Math.max(3, Math.round(objectHeight * 0.5));
  const drift = -Math.round(reach * 1.3);
  fillPolygon(
    context,
    [
      { x: x - width / 2, y: baseY },
      { x: x + width / 2, y: baseY },
      { x: x + width / 2 + drift, y: baseY + reach },
      { x: x - width / 2 + drift, y: baseY + reach },
    ],
    ramp(SOLID, 0),
  );
}

/** Штамповка кластеров только по настилу: выше диагонали парапета живёт город. */
function scatterDeck(
  context: CanvasRenderingContext2D,
  geometry: RoofGeometry,
  names: Parameters<typeof scatter>[5],
  color: string,
  density: number,
  seed: number,
  cell: number,
) {
  const { width, height } = geometry;
  for (let column = 0; column * cell < width; column += 1) {
    const x = column * cell;
    const top = capAt(geometry, x) + 5;
    for (let row = 0; row * cell < height - top; row += 1) {
      const y = top + row * cell;
      if (hash(seed, column, row) > density * (0.4 + hash(seed + 11, column >> 1, row >> 1) * 1.4)) continue;
      scatter(context, x, y, cell, cell, names, color, 1, seed + column * 7 + row, cell);
    }
  }
}

/**
 * Настил. Лунный свет ВПИСАН в бетон, а не положен поверх: свет отдельным слоем всегда
 * читается наклейкой. Луна высоко и справа, поэтому настил светлеет к зрителю и к правому
 * краю; дальний левый угол остаётся чёрным. Границы ступеней считаются аналитически —
 * три заливки на столбец вместо ста тысяч однопиксельных вызовов.
 */
export function drawDeck(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, height } = geometry;

  for (let x = 0; x < width; x += 1) {
    const top = capAt(geometry, x) + 1;
    const span = Math.max(1, height - top);
    const side = x / width;
    // lit = near * 0.62 + side * 0.38, где near — доля глубины настила.
    const boundary = (target: number) =>
      clamp(Math.round(top + ((target - side * 0.38) / 0.62) * span), top, height);
    const midY = boundary(0.4);
    const litY = boundary(0.74);
    context.fillStyle = ramp(SOLID, 0);
    context.fillRect(x, top, 1, midY - top);
    context.fillStyle = tint(SOLID, 1, HAZE_RAMP, 0.12);
    context.fillRect(x, midY, 1, litY - midY);
    context.fillStyle = tint(SOLID, 2, HAZE_RAMP, 0.2);
    context.fillRect(x, litY, 1, height - litY);
  }

  // Швы рубероида идут ПАРАЛЛЕЛЬНО парапету: любая горизонталь здесь спорила бы с
  // диагональю и разваливала перспективу.
  for (let index = 1; index <= 5; index += 1) {
    const offset = Math.round((height - geometry.capRight) * (index / 5) ** 1.6 * 0.9);
    let previous: { x: number; y: number } | null = null;
    for (let x = 0; x < width; x += 4) {
      const point = { x, y: capAt(geometry, x) + 4 + offset };
      if (previous && point.y < height - 1) line(context, previous.x, previous.y, point.x, point.y, ramp(SOLID, 0));
      previous = point;
    }
  }

  scatterDeck(context, geometry, ["dash2", "dash3"], ramp(SOLID, 0), 0.3, seed + 11, 6);
  scatterDeck(context, geometry, ["grit"], tint(SOLID, 3, HAZE_RAMP, 0.2), 0.16, seed + 17, 9);

  // Трещины: три ломаные, расходящиеся от левого края к зрителю.
  for (let crack = 0; crack < 3; crack += 1) {
    let x = Math.round(width * (0.1 + hash(seed, crack, 23) * 0.5));
    let y = capAt(geometry, x) + 10 + Math.round(hash(seed, crack, 29) * 20);
    for (let step = 0; step < 12; step += 1) {
      const nextX = x + 2 + Math.round(hash(seed + crack, step, 31) * 5);
      const nextY = y + Math.round(hash(seed + crack, step, 37) * 4) - 1;
      if (nextY >= height - 1 || nextY <= capAt(geometry, nextX) + 3) break;
      line(context, x, y, nextX, nextY, ramp(SOLID, 0));
      x = nextX;
      y = nextY;
    }
  }
}

/**
 * Парапет с широкой верхней гранью. Её положение задаёт опору для кота,
 * а затенение отделяет верхнюю грань от внутренней стороны парапета.
 */
export function drawParapet(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, unit, shedLeft } = geometry;
  const ledge = Math.round(unit * 0.17);
  const face = Math.round(unit * 0.34);

  for (let x = 0; x < width; x += 1) {
    const y = capAt(geometry, x);
    // Внутренняя грань смотрит на зрителя и в тень.
    context.fillStyle = ramp(SOLID, 0);
    context.fillRect(x, y, 1, ledge + face);
    // Полка: холодный кант, ровное лунное поле, тень у внутренней кромки.
    context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.62);
    context.fillRect(x, y, 1, 1);
    context.fillStyle = tint(SOLID, 2, HAZE_RAMP, 0.32);
    context.fillRect(x, y + 1, 1, ledge - 2);
    context.fillStyle = tint(SOLID, 1, HAZE_RAMP, 0.16);
    context.fillRect(x, y + ledge - 1, 1, 1);
    // Тёмная губа под полкой отделяет её от грани — без неё полка не читается объёмом.
    context.fillStyle = ramp(SOLID, 0);
    context.fillRect(x, y + ledge, 1, 2);
    context.fillStyle = ramp(SOLID, 1);
    context.fillRect(x, y + ledge + 2, 1, Math.round(face * 0.4));
  }

  // Швы блоков: короткие насечки поперёк пробега, шаг неровный.
  let x = Math.round(hash(seed, 0, 3) * 26);
  while (x < shedLeft) {
    const y = capAt(geometry, x);
    context.fillStyle = tint(SOLID, 1, HAZE_RAMP, 0.1);
    context.fillRect(x, y + 1, 1, ledge - 2);
    context.fillStyle = ramp(SOLID, 0);
    context.fillRect(x, y + ledge + 2, 1, face - 2);
    x += 20 + Math.round(hash(seed, x, 7) * 18);
  }
  // Небольшие неровности на кромке парапета.
  for (let chip = 0; chip < Math.max(4, Math.round(width / 80)); chip += 1) {
    const cx = Math.round(hash(seed, chip, 11) * shedLeft);
    const cy = capAt(geometry, cx);
    context.fillStyle = tint(SOLID, 1, HAZE_RAMP, 0.2);
    context.fillRect(cx, cy, 2 + Math.round(hash(seed, chip, 13) * 3), 1);
  }
}

/**
 * Надстройка справа: кирпичная стена и рифлёная штора. По референсу это вторая опора
 * композиции — вертикальная масса, уходящая за верх кадра и запирающая правый край.
 */
export function drawShed(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, shedLeft, unit } = geometry;
  const wallWidth = Math.round(unit * 0.5);
  const base = capAt(geometry, shedLeft) + Math.round(unit * 0.5);

  // Кирпичная стена, обращённая к зрителю: единственное большое тёплое поле в кадре,
  // но тусклое — это не свет, а собственный цвет кирпича под луной.
  for (let column = 0; column < wallWidth; column += 1) {
    const nx = column / Math.max(1, wallWidth - 1);
    const brick = nx < 0.12 ? ramp(SOLID, 0) : tint(SOLID, nx > 0.7 ? 3 : 2, WARM, 0.55);
    context.fillStyle = brick;
    context.fillRect(shedLeft + column, 0, 1, base);
  }
  for (let course = 0; course * 5 < base; course += 1) {
    if (hash(seed, course, 3) < 0.4) continue;
    context.fillStyle = tint(SOLID, 1, WARM, 0.4);
    context.fillRect(shedLeft + 2, course * 5, wallWidth - 3 - Math.round(hash(seed, course, 7) * 4), 1);
  }
  // Кант стены ловит луну.
  context.fillStyle = tint(SOLID, 3, WARM, 0.62);
  context.fillRect(shedLeft + wallWidth - 1, 0, 1, base);

  // Рифлёная штора: вертикальные полосы металла до правого края кадра.
  const shutterLeft = shedLeft + wallWidth;
  for (let column = shutterLeft; column < width; column += 1) {
    const phase = (column - shutterLeft) % 4;
    context.fillStyle = phase === 0 ? ramp(SOLID, 0) : phase === 2 ? tint(SOLID, 2, HAZE_RAMP, 0.24) : ramp(SOLID, 1);
    context.fillRect(column, 0, 1, base - Math.round(unit * 0.12));
  }
  context.fillStyle = ramp(SOLID, 0);
  context.fillRect(shutterLeft, base - Math.round(unit * 0.12), width - shutterLeft, Math.round(unit * 0.12));
  // Тёплое окно в стене: обжитая надстройка, а не декорация.
  const windowY = Math.round(base * 0.42);
  context.fillStyle = ramp(SOLID, 0);
  context.fillRect(shedLeft + 4, windowY - 1, Math.round(wallWidth * 0.5) + 2, Math.round(unit * 0.4) + 2);
  context.fillStyle = ramp(WARM, 1);
  context.fillRect(shedLeft + 5, windowY, Math.round(wallWidth * 0.5), Math.round(unit * 0.4));
  context.fillStyle = ramp(WARM, 2);
  context.fillRect(shedLeft + 5, windowY, Math.round(wallWidth * 0.5), 1);
}

/** Мелочь крыши: труба со шляпкой, ящик, свёрнутый кабель, тряпка и шляпа на парапете. */
export function drawRoofProps(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, height, unit, shedLeft } = geometry;

  // Вентиляционная труба у надстройки: вертикаль, за которую цепляется правый край.
  const pipeX = shedLeft - Math.round(unit * 0.5);
  const pipeBase = capAt(geometry, pipeX) + Math.round(unit * 0.55);
  const pipeHeight = Math.round(unit * 0.8);
  castShadow(context, pipeX, pipeBase, 5, pipeHeight);
  for (let column = 0; column < 5; column += 1) {
    context.fillStyle = ramp(SOLID, column === 0 ? 0 : column < 3 ? 1 : 2);
    context.fillRect(pipeX - 2 + column, pipeBase - pipeHeight, 1, pipeHeight);
  }
  context.fillStyle = ramp(SOLID, 0);
  context.fillRect(pipeX - 4, pipeBase - pipeHeight - 3, 9, 3);
  context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.3);
  context.fillRect(pipeX - 4, pipeBase - pipeHeight - 3, 9, 1);

  // Ящик у парапета: коту по грудь, тёмный контур и лунная крышка.
  const crateSize = Math.round(unit * 0.36);
  const crateX = Math.round(width * 0.78);
  const crateBase = capAt(geometry, crateX) + Math.round(unit * 0.75);
  castShadow(context, crateX, crateBase, crateSize, crateSize);
  context.fillStyle = ramp(SOLID, 0);
  context.fillRect(crateX - 1, crateBase - crateSize - 1, crateSize + 2, crateSize + 2);
  context.fillStyle = tint(SOLID, 1, WARM, 0.22);
  context.fillRect(crateX, crateBase - crateSize, crateSize, crateSize);
  context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.3);
  context.fillRect(crateX, crateBase - crateSize, crateSize, 1);
  context.fillStyle = tint(SOLID, 2, WARM, 0.2);
  context.fillRect(crateX + 1, crateBase - crateSize + 3, crateSize - 2, 1);
  context.fillRect(crateX + 1, crateBase - 4, crateSize - 2, 1);

  // Свёрнутый кабель: два овала друг в друге, лунная кромка сверху.
  const coilX = Math.round(width * 0.42);
  const coilY = height - Math.round(unit * 0.2);
  const coilR = Math.round(unit * 0.26);
  fillDisc(context, coilX, coilY, coilR, Math.round(coilR * 0.34), ramp(SOLID, 0));
  fillDisc(context, coilX, coilY - 1, coilR - 1, Math.max(2, Math.round(coilR * 0.26)), ramp(SOLID, 1));
  fillDisc(context, coilX, coilY - 1, Math.round(coilR * 0.4), 2, ramp(SOLID, 0));
  context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.26);
  context.fillRect(coilX - coilR + 2, coilY - 3, 5, 1);
  context.fillRect(coilX + coilR - 6, coilY - 3, 4, 1);

  // Тряпка, свисающая с парапета: пятно чужого цвета, как на референсе.
  const clothX = Math.round(width * 0.72);
  const clothTop = capAt(geometry, clothX);
  const clothW = Math.round(unit * 0.26);
  const clothH = Math.round(unit * 0.5);
  fillPolygon(
    context,
    [
      { x: clothX, y: clothTop - 1 },
      { x: clothX + clothW, y: clothTop + 1 },
      { x: clothX + clothW - 2, y: clothTop + clothH },
      { x: clothX + 2, y: clothTop + clothH - 3 },
    ],
    tint(NIGHT, 2, WARM, 0.3),
  );
  context.fillStyle = tint(NIGHT, 3, WARM, 0.36);
  context.fillRect(clothX + 1, clothTop - 1, clothW - 2, 1);
  context.fillStyle = ramp(NIGHT, 1);
  context.fillRect(clothX + 2, clothTop + clothH - 3, clothW - 4, 1);

  void seed;
}

/**
 * Лужа — главный тёплый акцент кадра, и сделана она по референсу: отражение не зеркало,
 * а ГОРИЗОНТАЛЬНЫЕ ШТРИХИ разной длины. Плюс холодная кромка в один пиксель по контуру,
 * кольца ряби и тёмный отпечаток кота. Источник тепла за кадром — этим она и живёт.
 */
export function drawPuddle(
  context: CanvasRenderingContext2D,
  geometry: RoofGeometry,
  frame: number,
  seed: number,
) {
  const { puddle, catX, shedLeft, unit } = geometry;
  const { x: centerX, y: centerY, radiusX, radiusY } = puddle;

  // Форма — не овал, а три сросшиеся лопасти: вода собирается в неровной впадине, и
  // ровный эллипс сразу читается наклейкой, а не лужей.
  const lobes = [
    { dx: -radiusX * 0.46, dy: -radiusY * 0.18, rx: radiusX * 0.6, ry: radiusY * 0.78 },
    { dx: radiusX * 0.24, dy: radiusY * 0.12, rx: radiusX * 0.72, ry: radiusY * 0.96 },
    { dx: radiusX * 0.78, dy: -radiusY * 0.3, rx: radiusX * 0.34, ry: radiusY * 0.52 },
  ];
  const spanAt = (row: number) => {
    let left = Number.POSITIVE_INFINITY;
    let right = Number.NEGATIVE_INFINITY;
    for (const lobe of lobes) {
      const ny = (row - lobe.dy) / lobe.ry;
      if (Math.abs(ny) >= 1) continue;
      const half = lobe.rx * Math.sqrt(1 - ny * ny);
      left = Math.min(left, lobe.dx - half);
      right = Math.max(right, lobe.dx + half);
    }
    if (left > right) return null;
    const jitter = (hash(seed, row + radiusY, 5) - 0.5) * radiusX * 0.1;
    return [Math.round(left + jitter), Math.round(right + jitter)] as const;
  };

  // Мокрый бетон ВОКРУГ воды: без него лужа лежит на сухом настиле отдельным предметом.
  // Кромка каймы рассыпается — вода не обрывается по линейке.
  for (let row = -radiusY - 5; row <= radiusY + 5; row += 1) {
    const span = spanAt(clamp(row, -radiusY + 1, radiusY - 1));
    if (!span) continue;
    const grow = 5 - Math.round(Math.abs(row) / Math.max(1, radiusY) * 3);
    for (const side of [-1, 1] as const) {
      for (let step = 0; step < grow; step += 1) {
        const x = centerX + (side < 0 ? span[0] - step : span[1] + step);
        if (step > grow - 3 && hash(seed + 9, row, step) > 0.5) continue;
        context.fillStyle = ramp(SOLID, 0);
        context.fillRect(x, centerY + row, 1, 1);
      }
    }
  }

  // Вода: дальняя кромка темнее ближней — она отражает зенит, а не город.
  for (let row = -radiusY; row <= radiusY; row += 1) {
    const span = spanAt(row);
    if (!span) continue;
    const near = (row + radiusY) / Math.max(1, radiusY * 2);
    context.fillStyle = near < 0.35 ? ramp(NIGHT, 0) : ramp(NIGHT, 1);
    context.fillRect(centerX + span[0], centerY + row, span[1] - span[0] + 1, 1);
  }

  // Отражение: перевёрнутая сцена штрихами. Полка парапета даёт светлую полосу, окно
  // надстройки — тёплый смаз, кот — тёмное пятно. Штрихи горизонтальные, разной длины.
  const capRow = -radiusY + Math.round(radiusY * 0.5);
  for (let row = -radiusY + 2; row < radiusY - 1; row += 1) {
    const span = spanAt(row);
    if (!span) continue;
    const inner = [span[0] + 2, span[1] - 2] as const;
    if (inner[1] - inner[0] < 6) continue;
    if (row === capRow || row === capRow + 1) {
      // Отражение полки: одна длинная холодная полоса, разорванная в двух местах.
      for (let x = inner[0]; x <= inner[1]; x += 1) {
        if (hash(seed + 11, row, x) > 0.86) continue;
        context.fillStyle = tint(SOLID, 2, HAZE_RAMP, 0.4);
        context.fillRect(centerX + x, centerY + row, 1, 1);
      }
      continue;
    }
    const heat = 1 - Math.abs(row - capRow - 4) / radiusY;
    const count = 1 + Math.floor(heat * 3);
    for (let dash = 0; dash < count; dash += 1) {
      if (hash(seed + 3, row + radiusY, dash) > 0.28 + heat * 0.5) continue;
      const dashWidth = 3 + Math.floor(hash(seed + 5, row, dash) * (4 + heat * 11));
      const axis = shedLeft - centerX - Math.round(unit * 0.4);
      const spread = Math.round((inner[1] - inner[0]) * 0.42);
      const offset = Math.round((hash(seed + 7, row, dash) * 2 - 1) * spread);
      const wobble = Math.round(Math.sin((frame + row * 3) * 0.18) * 1.4);
      const level: Level = heat > 0.76 ? 3 : heat > 0.46 ? 2 : 1;
      context.fillStyle = ramp(WARM, level);
      context.fillRect(centerX + clamp(axis + offset, inner[0], inner[1]) - Math.round(dashWidth / 2) + wobble, centerY + row, dashWidth, 1);
    }
  }

  // Отпечаток кота: тёмное пятно точно под ним, иначе он «висит» над водой.
  const catReflectX = catX + Math.round((centerX - catX) * 0.1);
  fillDisc(context, catReflectX, centerY - Math.round(radiusY * 0.45), Math.round(unit * 0.17), Math.round(radiusY * 0.5), ramp(NIGHT, 0));

  // Блик только на БЛИЖНЕЙ кромке и рваный: сплошной обод по кругу читается ободком чашки.
  for (let row = Math.round(radiusY * 0.2); row <= radiusY; row += 1) {
    const span = spanAt(row);
    if (!span) continue;
    for (const side of [-1, 1] as const) {
      const x = side < 0 ? span[0] : span[1];
      if (hash(seed + 13, row, side) > 0.62) continue;
      context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.55);
      context.fillRect(centerX + x, centerY + row, 1, 1);
    }
  }

  // Кольца ряби: две разорванные дуги, ход по такту.
  for (let ring = 0; ring < 2; ring += 1) {
    const life = (Math.floor(frame / 2) + ring * 9) % 18;
    const grow = life / 18;
    if (grow > 0.84) continue;
    const rx = Math.round(4 + grow * unit * 0.45);
    const ry = Math.max(1, Math.round(rx * 0.3));
    const originX = centerX + (ring === 0 ? Math.round(radiusX * 0.36) : -Math.round(radiusX * 0.28));
    const originY = centerY + (ring === 0 ? 2 : -3);
    context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.5);
    for (const side of [-1, 1] as const) {
      context.fillRect(originX - rx, originY + side * ry, Math.round(rx * 0.55), 1);
      context.fillRect(originX + Math.round(rx * 0.45), originY + side * ry, Math.round(rx * 0.55), 1);
    }
  }
}

/** Миска молока рядом с котом на крышке парапета: вторая тёплая крупица кадра. */
export function drawMilkBowl(context: CanvasRenderingContext2D, geometry: RoofGeometry, frame: number) {
  const { catX, unit } = geometry;
  const bowlWidth = Math.round(unit * 0.3);
  const x = catX + Math.round(unit * 0.66);
  const y = capAt(geometry, x) + 1;

  context.fillStyle = ramp(SOLID, 0);
  context.fillRect(x - Math.round(bowlWidth / 2) - 1, y - 4, bowlWidth + 2, 5);
  fillDisc(context, x, y - 2, Math.round(bowlWidth / 2), 3, ramp(SOLID, 1));
  fillDisc(context, x, y - 3, Math.round(bowlWidth / 2) - 1, 2, ramp(WARM, 3));
  const ripple = [0, 1, 1, 0, -1, -1][Math.floor(frame / 3) % 6];
  context.fillStyle = ramp(WARM, 2);
  context.fillRect(x - 2 + ripple, y - 3, 3, 1);
  context.fillStyle = tint(SOLID, 3, HAZE_RAMP, 0.4);
  context.fillRect(x - Math.round(bowlWidth / 2), y - 1, bowlWidth, 1);
}

/** Осенний сор: листья сдуты к парапету и в тень надстройки. Тёмная ржавчина, не апельсин. */
export function drawLeafDrifts(context: CanvasRenderingContext2D, geometry: RoofGeometry, seed: number) {
  const { width, height } = geometry;
  for (let column = 0; column * 6 < width; column += 1) {
    const x = column * 6;
    const top = capAt(geometry, x) + Math.round(geometry.unit * 0.32);
    // Листья держатся у самой грани парапета и у нижней кромки кадра — ветер сдувает
    // их к препятствиям, а не раскладывает ровным слоем по всей крыше.
    for (const [y, density] of [
      [top + 2, 0.4],
      [height - 6, 0.3],
    ] as const) {
      if (hash(seed, column, Math.round(y)) > density) continue;
      stamp(context, x + Math.round(hash(seed + 3, column, y) * 4), y, [[1, 0], [0, 1], [1, 1], [2, 1]], ramp(WARM, hash(seed + 7, column, y) > 0.7 ? 1 : 0));
    }
  }
}

/** Листья на ветру: целые позиции, две фазы кадра, тёмная ржавчина. */
export function drawFlyingLeaves(
  context: CanvasRenderingContext2D,
  geometry: RoofGeometry,
  frame: number,
  seed: number,
) {
  const { width, height } = geometry;
  const span = width + 40;
  for (let leaf = 0; leaf < 4; leaf += 1) {
    const offset = Math.floor(hash(seed, leaf, 3) * span);
    const x = ((offset + Math.floor(frame / 2) * (2 + (leaf % 3))) % span) - 20;
    const floor = capAt(geometry, x) + Math.round(geometry.unit * 0.4);
    const y = clamp(floor + Math.round(Math.sin((frame + leaf * 8) * 0.14) * 7), floor - 8, height - 3);
    stamp(context, x, y, frame % 4 < 2 ? [[0, 0], [1, 0], [1, 1]] : [[0, 0], [0, 1], [1, 1]], ramp(WARM, leaf % 3 === 0 ? 1 : 0));
  }
}

/** Пар из трубы: холодная завеса, поднимающаяся и растворяющаяся в ночи. */
export function drawVentSteam(
  context: CanvasRenderingContext2D,
  geometry: RoofGeometry,
  frame: number,
) {
  const { unit, shedLeft } = geometry;
  const x = shedLeft - Math.round(unit * 0.5);
  const baseY = capAt(geometry, x) + Math.round(unit * 0.55) - Math.round(unit * 0.86);
  for (let puff = 0; puff < 6; puff += 1) {
    const life = (frame * 2 + puff * 10) % 44;
    const t = life / 44;
    if (t > 0.86) continue;
    const radiusX = Math.round(3 + t * 9);
    const radiusY = Math.round(2 + t * 5);
    const sway = Math.round(Math.sin((frame + puff * 6) * 0.09) * (1 + t * 6));
    fillDisc(context, x + sway, baseY - life, radiusX, radiusY, ramp(NIGHT, t > 0.5 ? 2 : 3));
  }
}
