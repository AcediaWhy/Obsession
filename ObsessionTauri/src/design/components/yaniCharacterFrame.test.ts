import { describe, expect, it } from "vitest";
import * as THREE from "three";

import {
  lerpYaniFraming,
  YANI_FRAME_BOX,
  yaniCharacterViewFrame,
  yaniFrameCenterX,
  yaniFramingForScreen,
  type YaniFraming,
} from "./yaniCharacterFrame";

// Поле в дефолтном окне приложения: 1000×680 плюс оверскан параллакса (inset -32).
const FIELD_W = 1064;
const FIELD_H = 744;

describe("yaniCharacterFrame", () => {
  it("keeps the composition the old CSS transform produced", () => {
    // translateX(18%) от бокса 120% → центр кадра на 71.6% ширины поля;
    // на settings/profiles было translateX(21%) → 75.2%.
    expect(yaniFrameCenterX(yaniFramingForScreen("overview"))).toBeCloseTo(0.716, 5);
    expect(yaniFrameCenterX(yaniFramingForScreen("dpi"))).toBeCloseTo(0.716, 5);
    expect(yaniFrameCenterX(yaniFramingForScreen("settings"))).toBeCloseTo(0.752, 5);
    expect(yaniFrameCenterX(yaniFramingForScreen("profiles"))).toBeCloseTo(0.752, 5);
  });

  it("cuts the frustum so the visible window is exactly the field", () => {
    const frame = yaniCharacterViewFrame(FIELD_W, FIELD_H, yaniFramingForScreen("overview"));
    expect(frame.width).toBe(FIELD_W);
    expect(frame.height).toBe(FIELD_H);
    // Отрисованная рамка была 1.2×1.08 бокса, растянутого на 1.08.
    expect(frame.fullWidth).toBeCloseTo(1.296 * FIELD_W, 5);
    expect(frame.fullHeight).toBeCloseTo(1.1664 * FIELD_H, 5);
    // Окно начинается левее рамки (её левый край был на 6.8% поля) и ниже её верха.
    expect(frame.offsetX).toBeCloseTo(-0.068 * FIELD_W, 5);
    expect(frame.offsetY).toBeCloseTo(0.0832 * FIELD_H, 5);
  });

  it("never distorts the model: the cut frustum keeps the field aspect", () => {
    for (const screen of ["overview", "settings", "telegram"]) {
      const framing = yaniFramingForScreen(screen);
      const frame = yaniCharacterViewFrame(FIELD_W, FIELD_H, framing);
      // setViewOffset ставит aspect = fullWidth/fullHeight, затем сужает фрустум
      // отношениями width/fullWidth и height/fullHeight. Итог должен совпасть с
      // аспектом канваса, иначе модель растянет.
      const baseAspect = frame.fullWidth / frame.fullHeight;
      const cutAspect = baseAspect * (frame.width / frame.fullWidth) / (frame.height / frame.fullHeight);
      expect(cutAspect).toBeCloseTo(FIELD_W / FIELD_H, 10);
    }
  });

  it("interpolates the screen change and clamps outside 0..1", () => {
    const from = yaniFramingForScreen("overview");
    const to = yaniFramingForScreen("settings");
    expect(lerpYaniFraming(from, to, 0)).toEqual(from);
    expect(lerpYaniFraming(from, to, 1)).toEqual(to);
    expect(lerpYaniFraming(from, to, -3)).toEqual(from);
    expect(lerpYaniFraming(from, to, 7)).toEqual(to);
    expect(lerpYaniFraming(from, to, 0.5).shiftX).toBeCloseTo(0.195, 6);
    expect(lerpYaniFraming(from, to, 0.5).boxScale).toBeCloseTo(1.07, 6);
  });

  it("survives a degenerate field without producing a zero frustum", () => {
    const frame = yaniCharacterViewFrame(0, 0, yaniFramingForScreen("overview"));
    expect(frame.width).toBe(1);
    expect(frame.height).toBe(1);
    expect(frame.fullWidth).toBeGreaterThan(0);
    expect(frame.fullHeight).toBeGreaterThan(0);
  });
});

// Доказательство переноса кадрирования из CSS в камеру: одна и та же точка сцены
// должна попадать в одну и ту же точку поля. Слева — как было (камера на весь
// увеличенный канвас + CSS-трансформ поверх), справа — как стало (канвас по полю
// + вырез фрустума). Скриншот такого не гарантирует, а этот тест — гарантирует.
function makeCamera(): THREE.PerspectiveCamera {
  const camera = new THREE.PerspectiveCamera(29, 1, 0.1, 30);
  camera.position.set(3.4, 2.4, 3.4);
  camera.lookAt(0, 0.18, 0);
  camera.updateMatrixWorld();
  return camera;
}

/** Куда точка попадала в координатах поля при старой схеме. */
function fieldPointBeforeRefactor(
  point: THREE.Vector3,
  framing: YaniFraming,
): { x: number; y: number } {
  const camera = makeCamera();
  const canvasWidth = YANI_FRAME_BOX.width * FIELD_W;
  const canvasHeight = YANI_FRAME_BOX.height * FIELD_H;
  camera.aspect = canvasWidth / canvasHeight;
  camera.updateProjectionMatrix();
  const ndc = point.clone().project(camera);
  // Пиксели внутри канваса, затем layout-бокс канваса: inset:-4% -10%.
  const canvasX = ((ndc.x + 1) / 2) * canvasWidth - 0.1 * FIELD_W;
  const canvasY = ((1 - ndc.y) / 2) * canvasHeight - 0.04 * FIELD_H;
  // transform: translateX(shiftX от ширины бокса) scale(boxScale) вокруг центра.
  const centerX = 0.5 * FIELD_W;
  const centerY = 0.5 * FIELD_H;
  return {
    x: centerX + (canvasX - centerX) * framing.boxScale + framing.shiftX * canvasWidth,
    y: centerY + (canvasY - centerY) * framing.boxScale,
  };
}

/** Куда та же точка попадает теперь: канвас лежит ровно по полю. */
function fieldPointAfterRefactor(
  point: THREE.Vector3,
  framing: YaniFraming,
): { x: number; y: number } {
  const camera = makeCamera();
  const frame = yaniCharacterViewFrame(FIELD_W, FIELD_H, framing);
  camera.setViewOffset(
    frame.fullWidth,
    frame.fullHeight,
    frame.offsetX,
    frame.offsetY,
    frame.width,
    frame.height,
  );
  const ndc = point.clone().project(camera);
  return { x: ((ndc.x + 1) / 2) * FIELD_W, y: ((1 - ndc.y) / 2) * FIELD_H };
}

describe("yaniCharacterFrame — камера воспроизводит бывший CSS-трансформ", () => {
  // Габарит подогнанной модели ~1.58 юнита вокруг начала координат.
  const probes = [
    new THREE.Vector3(0, 0, 0),
    new THREE.Vector3(0.42, 0.61, -0.18),
    new THREE.Vector3(-0.37, -0.44, 0.29),
    new THREE.Vector3(0.11, 0.79, 0.33),
    new THREE.Vector3(-0.52, 0.08, -0.41),
  ];

  for (const screen of ["overview", "settings"]) {
    it(`puts every probe at the same field pixel on ${screen}`, () => {
      const framing = yaniFramingForScreen(screen);
      for (const probe of probes) {
        const before = fieldPointBeforeRefactor(probe, framing);
        const after = fieldPointAfterRefactor(probe, framing);
        expect(after.x).toBeCloseTo(before.x, 6);
        expect(after.y).toBeCloseTo(before.y, 6);
      }
    });
  }
});
