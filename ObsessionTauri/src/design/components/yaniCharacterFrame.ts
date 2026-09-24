// Кадрирование 3D-модели Yani рассчитывается через camera.setViewOffset.
// Canvas сохраняет размер поля; смещение и масштаб задаются параметрами камеры.

/** Размер виртуальной области камеры относительно видимого поля. */
export const YANI_FRAME_BOX = { width: 1.2, height: 1.08 } as const;

/** Горизонтальное смещение и масштаб кадра. */
export type YaniFraming = { shiftX: number; boxScale: number };

export type YaniViewFrame = {
  fullWidth: number;
  fullHeight: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
};

/** Параметры кадрирования для экрана приложения. */
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
 * Координата центра кадра по X в долях ширины поля.
 */
export function yaniFrameCenterX(framing: YaniFraming): number {
  return 0.5 + framing.shiftX * YANI_FRAME_BOX.width;
}

/**
 * Параметры `camera.setViewOffset`: полная область камеры и видимый участок
 * в пикселях поля. Метод обновляет `camera.aspect` по полной области.
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
  // Положение полной области после масштабирования и горизонтального сдвига.
  const left = (yaniFrameCenterX(framing) - 0.5 * YANI_FRAME_BOX.width * framing.boxScale) * width;
  const top = (0.5 - 0.5 * YANI_FRAME_BOX.height * framing.boxScale) * height;
  return { fullWidth, fullHeight, offsetX: -left, offsetY: -top, width, height };
}
