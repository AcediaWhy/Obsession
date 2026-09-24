import { clamp } from "./pixelCore";

// Геометрия крыши рассчитана для вида сверху: парапет идёт по диагонали,
// а надстройка занимает правый край. Размеры предметов зависят от UNIT —
// высоты силуэта кота. Положение луны общее для освещения и теней сцены.

export type RoofGeometry = {
  width: number;
  height: number;
  /** Рост кота в арт-пикселях: единица масштаба всей сцены. */
  unit: number;
  /** Низ неба = верх городского ковра. */
  skyBottom: number;
  /** Низ ковра дальнего города: ниже начинаются ближние чёрные массы. */
  carpetBottom: number;
  /** Левый край надстройки: правее неё крыша уходит в тень стены. */
  shedLeft: number;
  /** Крышка парапета в правом конце видимого пробега. */
  capRight: number;
  /** Точка выхода парапета за левый край кадра. */
  capLeft: { x: number; y: number };
  /** Луна — единственный источник света в кадре. */
  moon: { x: number; y: number; radius: number };
  catX: number;
  catFeet: number;
  puddle: { x: number; y: number; radiusX: number; radiusY: number };
};

/** Рост кота: спрайт 64×64 с полом на 54, силуэт занимает примерно 50 пикселей. */
const UNIT = 50;

/** Крышка парапета в точке x. Правее надстройки парапет скрыт, поэтому линия обрезана. */
export function capAt(geometry: RoofGeometry, x: number) {
  const { shedLeft, capRight, capLeft } = geometry;
  const t = clamp((shedLeft - x) / Math.max(1, shedLeft - capLeft.x), 0, 1);
  return Math.round(capRight + t * (capLeft.y - capRight));
}

export function roofGeometry(width: number, height: number): RoofGeometry {
  const skyBottom = Math.round(height * 0.28);
  // Диагональ ограничивает крышу правой нижней частью кадра и оставляет
  // место для города слева.
  const capRight = Math.round(height * 0.5);
  const shedLeft = Math.round(width * 0.82);
  const capLeft = { x: Math.round(width * 0.4), y: height };

  const base: RoofGeometry = {
    width,
    height,
    unit: UNIT,
    skyBottom,
    carpetBottom: Math.round(height * 0.62),
    shedLeft,
    capRight,
    capLeft,
    // Луна расположена выше карточек интерфейса, которые перекрывают фон.
    moon: {
      x: Math.round(width * 0.7),
      y: Math.round(skyBottom * 0.34),
      radius: clamp(Math.round(height * 0.016), 5, 12),
    },
    catX: Math.round(width * 0.62),
    catFeet: 0,
    puddle: { x: 0, y: 0, radiusX: 0, radiusY: 0 },
  };

  // Размещаем кота на светлой кромке парапета для контраста с фоном.
  base.catFeet = capAt(base, base.catX) + Math.round(UNIT * 0.09);
  // Лужа занимает открытую часть настила ниже и правее кота.
  base.puddle = {
    x: Math.round(width * 0.7),
    y: Math.round(height * 0.9),
    radiusX: Math.round(width * 0.17),
    radiusY: Math.round(height * 0.065),
  };
  return base;
}

