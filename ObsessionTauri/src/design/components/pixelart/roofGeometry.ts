import { clamp } from "./pixelCore";

// Геометрия кадра собрана по референсу: камера стоит НА крыше и смотрит вниз на город,
// парапет уходит диагональю из правого края в левый нижний угол, справа кадр держит
// вертикальная масса надстройки. Диагональ — главный инструмент глубины: горизонтальная
// полоса парапета, которая была здесь раньше, читалась «стеной на уровне глаз».
//
// Второе правило: масштаб всех предметов задан в ЕДИНИЦАХ РОСТА КОТА, а не в долях
// кадра. Кот — мера сцены, поэтому предметы соотносятся друг с другом, а не с окном.
//
// Третье: источник света ОДИН и он в небе — луна. Её положение живёт здесь, а не в
// рисовальщике неба, потому что от него зависят все грани и все тени на крыше.

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
  // Парапет выходит за кадр не в левом углу, а на 40% ширины: тогда крыша занимает
  // только правый нижний клин, а весь левый низ достаётся ГОРОДУ. Раньше настил
  // забирал почти всю открытую полосу и она стояла пустой.
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
    // Луна поднята выше, чем «красиво»: на штатной высоте она попадала прямо под
    // карточку «Службы защиты» и в боевом окне пропадала за стеклом.
    moon: {
      x: Math.round(width * 0.7),
      y: Math.round(skyBottom * 0.34),
      radius: clamp(Math.round(height * 0.016), 5, 12),
    },
    catX: Math.round(width * 0.62),
    catFeet: 0,
    puddle: { x: 0, y: 0, radiusX: 0, radiusY: 0 },
  };

  // Кот сидит НА полке парапета: его силуэт обязан лечь на самую светлую полосу
  // города, иначе тёмный зверь на тёмном фоне просто исчезает.
  base.catFeet = capAt(base, base.catX) + Math.round(UNIT * 0.09);
  // Лужа лежит в клине настила, ниже и правее кота — там, где в боевом окне под
  // панелями ничего нет. Это главный тёплый акцент кадра.
  base.puddle = {
    x: Math.round(width * 0.7),
    y: Math.round(height * 0.9),
    radiusX: Math.round(width * 0.17),
    radiusY: Math.round(height * 0.065),
  };
  return base;
}


