import { describe, expect, it } from "vitest";

import { screenVariants } from "./screenTransition";
import { FOCUS_EXIT } from "./focusTransition";

// Варианты направления — функции от custom; резолвим их так же, как framer.
function resolve(variant: unknown, dir: number): Record<string, unknown> {
  const value = typeof variant === "function" ? (variant as (custom: number) => unknown)(dir) : variant;
  return value as Record<string, unknown>;
}

describe("screenVariants", () => {
  it("уходящий экран не перехватывает клики по входящему", () => {
    // Регрессия: экран уходит через opacity у панелей, а нулевая прозрачность
    // НЕ отключает хит-тестинг. Без явного pointerEvents невидимый экран
    // оставался кликабельным щитом, и в Настройках переставала меняться тема.
    expect(resolve(screenVariants.exit, 1).pointerEvents).toBe("none");
    expect(resolve(screenVariants.exit, -1).pointerEvents).toBe("none");
  });

  it("входящий экран интерактивен после прерванного выхода", () => {
    // Быстрый возврат на тот же раздел оживляет ТОТ ЖЕ элемент: если enter/center
    // не вернут pointerEvents, на нём останется "none" из exit.
    expect(resolve(screenVariants.enter, 1).pointerEvents).toBe("auto");
    expect(resolve(screenVariants.center, 1).pointerEvents).toBe("auto");
  });

  it("сохраняет геометрию и backdrop стекла на всех фазах", () => {
    for (const phase of [screenVariants.enter, screenVariants.center, screenVariants.exit]) {
      for (const direction of [-1, 1]) {
        const variant = resolve(phase, direction);
        for (const property of ["x", "y", "scale", "opacity", "filter"]) {
          expect(variant).not.toHaveProperty(property);
        }
      }
    }
  });

  it("держит уходящий экран только до завершения короткого выхода содержимого", () => {
    expect(resolve(screenVariants.enter, 1)["--route-presence"]).toBe(1);
    expect(resolve(screenVariants.center, 1)["--route-presence"]).toBe(1);
    expect(resolve(screenVariants.exit, 1)["--route-presence"]).toBe(0);
    expect(resolve(screenVariants.exit, 1).transition).toMatchObject({ duration: FOCUS_EXIT / 1000 });
  });
});
