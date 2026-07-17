import { describe, expect, it, vi } from "vitest";

import {
  normalizeRainPointer,
  runRainFrame,
  smoothRainValue,
  type RainFramePhases,
} from "./rainFramePipeline";

describe("Rain frame pipeline", () => {
  it("keeps parallax smoothing stable across refresh rates", () => {
    const advance = (fps: number) => {
      let value = 0;
      for (let frame = 0; frame < fps; frame += 1) {
        value = smoothRainValue(value, 1, 1 / fps);
      }
      return value;
    };

    expect(advance(60)).toBeCloseTo(advance(120), 10);
    expect(advance(60)).toBeCloseTo(1 - Math.pow(0.94, 60), 10);
  });

  it("normalizes pointer input against the CSS client rect", () => {
    const rect = { left: 100, top: 50, width: 800, height: 400 };

    expect(normalizeRainPointer(500, 250, rect)).toEqual({ x: 0, y: 0 });
    expect(normalizeRainPointer(0, 1000, rect)).toEqual({ x: -1, y: 1 });
  });

  it("runs input, smoothing, simulation, texture upload and draw in order", () => {
    const order: string[] = [];
    const phases: RainFramePhases = {
      input: vi.fn(() => order.push("input")),
      smooth: vi.fn((dt) => order.push("smooth:" + dt.toFixed(3))),
      simulate: vi.fn((dt) => order.push("simulate:" + dt.toFixed(3))),
      uploadTexture: vi.fn(() => order.push("texture")),
      draw: vi.fn(() => order.push("draw")),
    };

    runRainFrame(phases, 1 / 60);

    expect(order).toEqual(["input", "smooth:0.017", "simulate:0.017", "texture", "draw"]);
  });
});
