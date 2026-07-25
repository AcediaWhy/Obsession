import { describe, expect, it } from "vitest";

import { RainMistSim } from "./mistSim";

describe("RainMistSim", () => {
  it("grows condensation toward the weather cap", () => {
    const mist = new RainMistSim(16, 9, 0.1);
    for (let frame = 0; frame < 90 * 30; frame += 1) mist.step(1 / 30, 1, []);

    // Потолок при activity=1 — 0.88; за полторы минуты почти насыщение.
    expect(mist.mean()).toBeGreaterThan(0.7);
    expect(mist.mean()).toBeLessThanOrEqual(0.88 + 1e-6);
  });

  it("keeps growth stable across refresh rates", () => {
    const run = (fps: number) => {
      const mist = new RainMistSim(8, 8, 0.2);
      for (let frame = 0; frame < fps * 20; frame += 1) mist.step(1 / fps, 0.5, []);
      return mist.mean();
    };

    expect(run(30)).toBeCloseTo(run(120), 2);
  });

  it("wipes a circle and regrows it afterwards", () => {
    const mist = new RainMistSim(32, 18, 0.6);
    const before = mist.levelAt(0.5, 0.5);
    mist.step(1 / 60, 0.5, [{ x: 0.5, y: 0.5, radius: 0.12 }]);

    const wiped = mist.levelAt(0.5, 0.5);
    expect(wiped).toBeLessThan(before * 0.25);
    // Угол сетки протирка не задела.
    expect(mist.levelAt(0, 0)).toBeGreaterThan(0.5);

    for (let frame = 0; frame < 60 * 60; frame += 1) mist.step(1 / 60, 0.5, []);
    expect(mist.levelAt(0.5, 0.5)).toBeGreaterThan(0.4);
  });

  it("survives resize by keeping the mean level", () => {
    const mist = new RainMistSim(16, 9, 0.33);
    mist.resize(24, 12);

    expect(mist.size).toEqual({ cols: 24, rows: 12 });
    expect(mist.mean()).toBeCloseTo(0.33, 5);
  });

  it("writes bytes for the full RGBA grid", () => {
    const mist = new RainMistSim(4, 3, 0.5);
    const data = new Uint8ClampedArray(4 * 3 * 4);
    mist.writeTo(data);

    expect(data[0]).toBe(128);
    expect(data[3]).toBe(255);
    expect(data[data.length - 1]).toBe(255);
  });
});
