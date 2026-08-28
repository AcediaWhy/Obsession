// Кадрирование 3D-Yani.
//
// Раньше канвас модели был крупнее поля (`inset:-4% -10%`, `120%×108%`) и ещё раз
// растягивался CSS-трансформом `translateX(18%) scale(1.08)`, после чего
// `overflow:hidden` поля обрезал лишнее. Считая клип, на экран попадало 71.9%
// отрисованного по X и 85.7% по Y — то есть **38% пикселей рисовались в мусор**,
// а картинка вдобавок апскейлилась (0.93 device px на пиксель экрана).
//
// Теперь канвас лежит ровно по полю, а ту же композицию даёт сдвиг фрустума
// (`camera.setViewOffset`): та же точка сцены оказывается в той же точке экрана,
// но пиксели рисуются только те, что видно.

/** Геометрия бывшей CSS-рамки канваса — источник всех коэффициентов ниже. */
export const YANI_FRAME_BOX = { width: 1.2, height: 1.08 } as const;

/** Композиция кадра: сдвиг вправо и зум. Бывшие `translateX()` и `scale()`. */
export type YaniFraming = { shiftX: number; boxScale: number };

export type YaniViewFrame = {
  fullWidth: number;
  fullHeight: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
};

/** Бывшие CSS-правила `[data-screen]` для `.yani-character-field__model`. */
export function yaniFramingForScreen(screen: string): YaniFraming {
  if (screen === "settings" || screen === "profiles") return { shiftX: 0.21, boxScale: 1.06 };
  return { shiftX: 0.18, boxScale: 1.08 };
}

export function lerpYaniFraming(from: YaniFraming, to: YaniFraming, t: number): YaniFraming {
  const clamped = t <= 0 ? 0 : t >= 1 ? 1 : t;
  return {
    shiftX: from.shiftX + (to.shiftX - from.shiftX) * clamped,
    boxScale: from.boxScale + (to.boxScale - from.boxScale) * clamped,
  };
}

/**
 * Куда попадает центр кадра камеры в координатах поля (0..1). Композиция «не
 * поехала» ровно тогда, когда это число совпадает со старым CSS: 0.716 на
 * обычных экранах и 0.752 на settings/profiles.
 */
export function yaniFrameCenterX(framing: YaniFraming): number {
  return 0.5 + framing.shiftX * YANI_FRAME_BOX.width;
}

/**
 * Подпрямоугольник фрустума для `camera.setViewOffset`. `setViewOffset` сам
 * перетирает `camera.aspect` отношением fullWidth/fullHeight, поэтому обе
 * величины возвращаются в пикселях поля — их отношение равно аспекту бывшего
 * layout-бокса канваса, а вырезаемое окно ровно совпадает с полем.
 */
export function yaniCharacterViewFrame(
  fieldWidth: number,
  fieldHeight: number,
  framing: YaniFraming,
): YaniViewFrame {
  const width = Math.max(1, fieldWidth);
  const height = Math.max(1, fieldHeight);
  const fullWidth = YANI_FRAME_BOX.width * framing.boxScale * width;
  const fullHeight = YANI_FRAME_BOX.height * framing.boxScale * height;
  // Левый/верхний край бывшей отрисованной рамки в координатах поля: центр бокса
  // совпадал с центром поля, дальше scale вокруг центра и сдвиг вправо.
  const left = (yaniFrameCenterX(framing) - 0.5 * YANI_FRAME_BOX.width * framing.boxScale) * width;
  const top = (0.5 - 0.5 * YANI_FRAME_BOX.height * framing.boxScale) * height;
  return { fullWidth, fullHeight, offsetX: -left, offsetY: -top, width, height };
}
