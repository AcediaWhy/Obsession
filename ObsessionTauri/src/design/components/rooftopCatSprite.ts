import type { ObsessionVisualPhase } from "../obsessionVisualState";
import {
  clamp,
  fillDisc,
  fillPolygon,
  HAZE_RAMP,
  type Level,
  line,
  ramp,
  SOLID,
  tint,
  WARM,
  withPlane,
} from "./pixelart/pixelCore";

// Кот на крыше. Спрайт живёт в буфере 64×64 и вклеивается и в мир, и в кнопку-ядра,
// поэтому кот в кадре и кот в интерфейсе — один и тот же зверь.
//
// Пропорции ЧИБИ, а не анатомические: голова почти во всю ширину корпуса, уши высотой в
// голову, глаза — огромные круглые кольца, корпус — простая тёмная масса почти без
// внутренних деталей. Всё время до этого я тянул кота к анатомии кошки и проигрывал:
// на 64 пикселях побеждает не правильность, а читаемость силуэта и лица.
//
// Силуэт задан РУКАМИ, интервалами на каждую строку. Свет — луна сверху-справа: холодная
// грань справа, тень слева, тонкая холодная кромка по спине.

export type RoofCatDetail = "base" | "hero";

const SIZE = 64;
const GROUND = 54;

// Голова: y 14…30. Ширина 23 — почти как корпус, это и есть чиби-пропорция.
const HEAD_TOP = 14;
const HEAD: readonly (readonly [number, number])[] = [
  [26, 38], [24, 40], [23, 41], [22, 42], [22, 42], [21, 43],
  [21, 43], [21, 43], [21, 43], [21, 43], [21, 43], [22, 42],
  [22, 42], [23, 41], [24, 40], [26, 38], [28, 36],
];

// Корпус: y 28…53. Каплей, без раздутого основания.
const BODY_TOP = 28;
const BODY: readonly (readonly [number, number])[] = [
  [27, 37], [26, 38], [25, 39], [24, 40], [24, 40], [23, 41],
  [23, 41], [22, 42], [22, 42], [21, 43], [21, 43], [21, 43],
  [20, 44], [20, 44], [20, 44], [19, 44], [19, 45], [19, 45],
  [19, 45], [19, 45], [19, 45], [19, 45], [20, 45], [20, 44],
  [21, 44], [22, 43],
];

type Pose = {
  /** Наклон к миске: голова опускается на столько пикселей. */
  lean: number;
  /** Поворот головы в пикселях. */
  turn: number;
  /** Уши: -1 прижаты, 0 обычно, 1 навострены. */
  ears: -1 | 0 | 1;
  /** Глаза: 0 зажмурен, 1 обычно, 2 расширены. */
  eyes: 0 | 1 | 2;
  /** Ход хвоста. */
  tail: number;
};

const POSE: Record<ObsessionVisualPhase, Pose> = {
  idle: { lean: 0, turn: 0, ears: 0, eyes: 1, tail: 1 },
  engaging: { lean: 0, turn: 1, ears: 1, eyes: 1, tail: 3 },
  scanning: { lean: 0, turn: 2, ears: 1, eyes: 2, tail: 4 },
  focused: { lean: 5, turn: 1, ears: 0, eyes: 1, tail: 0 },
  fault: { lean: 0, turn: -2, ears: -1, eyes: 0, tail: -2 },
};

/**
 * Шерсть. Кот бурый, но освещён ЛУНОЙ: тёплое живёт только в тенях (там это собственный
 * цвет меха), самая светлая ступень уходит в холод. Отдельной рампы под кота в
 * шестнадцати цветах нет, и она не нужна.
 */
function fur(level: Level) {
  if (level === 0) return tint(SOLID, 0, WARM, 0.34);
  if (level === 1) return tint(SOLID, 1, WARM, 0.4);
  if (level === 2) return tint(SOLID, 2, WARM, 0.24);
  return tint(SOLID, 3, HAZE_RAMP, 0.5);
}

/**
 * Заливка силуэта. Контраст нарочно низкий: у чиби-кота корпус — почти плоская тёмная
 * масса, вся работа достаётся силуэту и лицу. Как только внутри корпуса появляется
 * лепка, кот превращается в комок.
 */
function shadeSpan(
  context: CanvasRenderingContext2D,
  spans: readonly (readonly [number, number])[],
  top: number,
  dx: number,
  dy: number,
  contrast: number,
) {
  const rows = Math.max(1, spans.length - 1);
  for (const [index, [start, end]] of spans.entries()) {
    const y = top + index + dy;
    const width = Math.max(1, end - start);
    const ny = index / rows;
    for (let x = start; x <= end; x += 1) {
      const nx = (x - start) / width;
      const value = ((nx - 0.58) * 1.1 + (0.42 - ny) * 0.4) * contrast;
      let level: Level = value > 0.2 ? 2 : value > 0.02 ? 1 : 0;
      if (x === start || x === end) level = 0;
      context.fillStyle = fur(level);
      context.fillRect(x + dx, y, 1, 1);
    }
  }
}

/** Холодная кромка по контуру: небо за спиной кота есть всегда, но не сплошняком. */
function drawRim(
  context: CanvasRenderingContext2D,
  spans: readonly (readonly [number, number])[],
  top: number,
  dx: number,
  dy: number,
  side: "left" | "right",
  from: number,
  to: number,
) {
  context.fillStyle = fur(3);
  for (let index = from; index <= to && index < spans.length; index += 1) {
    if (index % 4 === 3) continue;
    const [start, end] = spans[index];
    context.fillRect((side === "left" ? start + 1 : end - 1) + dx, top + index + dy, 1, 1);
  }
}

/** Уши высотой в голову — половина обаяния чиби-кота живёт здесь. */
function drawEars(context: CanvasRenderingContext2D, pose: Pose, dx: number, dy: number) {
  const lift = pose.ears * 3;
  const flat = pose.ears < 0;
  const tipY = (flat ? 12 : 3) + dy - lift;
  // Левое ухо.
  fillPolygon(
    context,
    [
      { x: 23 + dx, y: 23 + dy },
      { x: 32 + dx, y: 16 + dy },
      { x: flat ? 15 + dx : 20 + dx, y: tipY },
    ],
    fur(0),
  );
  fillPolygon(
    context,
    [
      { x: 25 + dx, y: 21 + dy },
      { x: 30 + dx, y: 17 + dy },
      { x: flat ? 18 + dx : 22 + dx, y: tipY + 5 },
    ],
    ramp(WARM, 0),
  );
  // Правое ухо обращено к луне: светлее, с холодной кромкой по внешнему краю.
  fillPolygon(
    context,
    [
      { x: 32 + dx, y: 16 + dy },
      { x: 41 + dx, y: 23 + dy },
      { x: flat ? 49 + dx : 44 + dx, y: tipY },
    ],
    fur(1),
  );
  fillPolygon(
    context,
    [
      { x: 34 + dx, y: 17 + dy },
      { x: 39 + dx, y: 21 + dy },
      { x: flat ? 46 + dx : 42 + dx, y: tipY + 5 },
    ],
    ramp(WARM, 1),
  );
  line(context, 41 + dx, 23 + dy, (flat ? 49 : 44) + dx, tipY, fur(3));
}

/**
 * Лицо. Глаза — огромные кольца: светлый обод и тёмный зрачок. Именно они и делают кота
 * котом на таком размере; всё, что мельче трёх пикселей, на 64px не читается.
 */
function drawFace(
  context: CanvasRenderingContext2D,
  pose: Pose,
  dx: number,
  dy: number,
  frame: number,
  detail: RoofCatDetail,
) {
  const blink = frame % 37 === 12 || frame % 37 === 13;
  const open = blink ? 0 : pose.eyes;
  const eyeY = 23 + dy;
  for (const eyeX of [27 + dx, 37 + dx]) {
    if (open === 0) {
      context.fillStyle = fur(0);
      context.fillRect(eyeX - 3, eyeY, 7, 1);
      continue;
    }
    const radius = open === 2 ? 4 : 3;
    fillDisc(context, eyeX, eyeY, radius, radius, fur(0));
    fillDisc(context, eyeX, eyeY, radius - 1, radius - 1, ramp(WARM, 3));
    fillDisc(context, eyeX, eyeY, Math.max(1, radius - 2), Math.max(1, radius - 2), fur(0));
    // Блик: один пиксель на верхней левой кромке зрачка.
    context.fillStyle = ramp(WARM, 3);
    context.fillRect(eyeX - 1, eyeY - 1, 1, 1);
  }
  // Нос и рот: три пикселя и две короткие линии, больше на этом размере не нужно.
  context.fillStyle = ramp(WARM, 1);
  context.fillRect(31 + dx, 29 + dy, 3, 1);
  context.fillStyle = fur(0);
  context.fillRect(32 + dx, 30 + dy, 1, 2);
  context.fillRect(30 + dx, 31 + dy, 2, 1);
  context.fillRect(33 + dx, 31 + dy, 2, 1);
  if (detail === "hero") {
    for (const side of [-1, 1] as const) {
      line(context, 32 + dx + side * 5, 30 + dy, 32 + dx + side * 13, 27 + dy, fur(3));
    }
  }
}

/** Хвост поднят и загнут — тонкая линия, а не сосиска вокруг лап. */
function drawTail(context: CanvasRenderingContext2D, pose: Pose, frame: number) {
  const flick = Math.round(Math.sin(frame * 0.22) * pose.tail);
  const path: readonly (readonly [number, number])[] = [
    [44, 50], [47, 49], [49, 46], [50, 42], [49, 38], [47, 35], [44, 33],
  ];
  for (const [index, [x, y]] of path.entries()) {
    const shift = index > 2 ? Math.round((flick * (index - 2)) / 4) : 0;
    const thickness = index < 2 ? 4 : 3;
    context.fillStyle = fur(0);
    context.fillRect(x - 1, y - 1 + shift, thickness + 2, thickness + 2);
  }
  for (const [index, [x, y]] of path.entries()) {
    const shift = index > 2 ? Math.round((flick * (index - 2)) / 4) : 0;
    const thickness = index < 2 ? 4 : 3;
    context.fillStyle = index > 3 ? fur(2) : fur(1);
    context.fillRect(x, y + shift, thickness, thickness);
  }
  // Кончик светлее всего: он выше и ближе к луне.
  context.fillStyle = fur(3);
  context.fillRect(44, 33 + Math.round((flick * 4) / 4), 2, 2);
}

/** Передние лапы: две подушечки с тёмным зазором. Пальцы намечены двумя пикселями. */
function drawPaws(context: CanvasRenderingContext2D) {
  for (const [index, x] of [25, 34].entries()) {
    context.fillStyle = fur(0);
    context.fillRect(x - 1, 47, 8, 7);
    context.fillStyle = fur(index === 1 ? 2 : 1);
    context.fillRect(x, 48, 6, 5);
    context.fillStyle = fur(index === 1 ? 3 : 2);
    context.fillRect(x, 48, 6, 1);
    context.fillStyle = fur(0);
    context.fillRect(x + 2, 51, 1, 3);
    context.fillRect(x + 4, 51, 1, 3);
  }
}

export function drawRoofCatSprite(
  context: CanvasRenderingContext2D,
  phase: ObsessionVisualPhase,
  frame: number,
  detail: RoofCatDetail = "hero",
) {
  context.clearRect(0, 0, SIZE, SIZE);
  withPlane(3, () => {
    const pose = POSE[phase];
    const breath = phase === "fault" ? 0 : Math.floor(frame / 4) % 2;
    const sipping = pose.lean > 0 && Math.floor(frame / 6) % 3 !== 2;
    const dy = breath + (sipping ? pose.lean : 0);
    const dx = pose.turn;

    // Своя тень: луна высоко, поэтому тень короткая и уходит влево.
    fillPolygon(
      context,
      [
        { x: 20, y: GROUND },
        { x: 46, y: GROUND },
        { x: 42, y: GROUND + 3 },
        { x: 16, y: GROUND + 3 },
      ],
      ramp(SOLID, 0),
    );

    drawTail(context, pose, frame);
    shadeSpan(context, BODY, BODY_TOP, 0, 0, 0.5);
    drawRim(context, BODY, BODY_TOP, 0, 0, "left", 3, 18);
    drawPaws(context);
    drawEars(context, pose, dx, dy);
    shadeSpan(context, HEAD, HEAD_TOP, dx, dy, 0.42);
    drawRim(context, HEAD, HEAD_TOP, dx, dy, "right", 1, 9);
    drawFace(context, pose, dx, dy, frame, detail);

    if (phase === "fault") {
      for (const [index, x] of [24, 28, 32].entries()) {
        context.fillStyle = fur(2);
        context.fillRect(x, 30 - (index % 2), 1, 3);
      }
    }
    if (phase === "scanning") {
      context.fillStyle = fur(3);
      context.fillRect(44 + dx, 3 + dy + (frame % 2), 1, 1);
    }
  });
}

/** Размер спрайта в арт-пикселях: сцене нужно знать, куда сажать лапы. */
export const ROOF_CAT_SIZE = SIZE;
export const ROOF_CAT_GROUND = GROUND;

/** Кратный масштаб для стенда: 64 → 64·n, иначе ближайший сосед даёт неровности. */
export function roofCatScale(target: number) {
  return clamp(Math.max(1, Math.round(target / SIZE)), 1, 8);
}
