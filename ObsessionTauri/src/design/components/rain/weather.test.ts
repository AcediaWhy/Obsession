import { describe, expect, it } from "vitest";

import { RainWeatherModel, type RainRandom } from "./weather";

function sequence(values: number[]): RainRandom {
  let index = 0;
  return () => values[Math.min(index++, values.length - 1)] ?? 0;
}

function advance(model: RainWeatherModel, seconds: number, fps: number, active: boolean) {
  const frames = Math.round(seconds * fps);
  const dt = seconds / frames;
  let snapshot = model.step(0, { active, reducedMotion: false });
  for (let frame = 0; frame < frames; frame += 1) {
    snapshot = model.step(dt, { active, reducedMotion: false });
  }
  return snapshot;
}

describe("RainWeatherModel", () => {
  it("keeps activity transitions stable across refresh rates", () => {
    const at60 = advance(new RainWeatherModel(false, () => 1), 1.1, 60, true);
    const at165 = advance(new RainWeatherModel(false, () => 1), 1.1, 165, true);

    expect(at60.activity).toBeCloseTo(at165.activity, 5);
    expect(at60.activity).toBeCloseTo(1 - Math.exp(-1), 2);
    expect(advance(new RainWeatherModel(false, () => 1), 3, 60, true).activity).toBeGreaterThan(0.9);
  });

  it("maps activity to the approved bounded weather multipliers", () => {
    const idle = new RainWeatherModel(false, () => 1).step(0, {
      active: false,
      reducedMotion: false,
    });
    const active = new RainWeatherModel(true, () => 1).step(0, {
      active: true,
      reducedMotion: false,
    });

    expect(idle).toMatchObject({ rainDensity: 1, rainSpeed: 1, trailRate: 1 });
    expect(active).toMatchObject({ rainDensity: 1.35, rainSpeed: 1.25, trailRate: 1.3 });
  });

  it("schedules lightning independently from application activity", () => {
    const idle = new RainWeatherModel(false, sequence([0, 0]));
    const active = new RainWeatherModel(true, sequence([0, 0]));
    const idleValues: number[] = [];
    const activeValues: number[] = [];

    for (let frame = 0; frame < 46 * 60; frame += 1) {
      idleValues.push(idle.step(1 / 60, { active: false, reducedMotion: false }).lightning);
      activeValues.push(active.step(1 / 60, { active: true, reducedMotion: false }).lightning);
    }

    expect(idleValues).toEqual(activeValues);
    expect(Math.max(...idleValues)).toBeGreaterThan(0.2);
    expect(Math.max(...idleValues)).toBeLessThanOrEqual(0.28);
  });

  it("fully disables lightning and snaps state under reduced motion", () => {
    const model = new RainWeatherModel(false, () => 0);
    let peak = 0;
    for (let frame = 0; frame < 130 * 10; frame += 1) {
      const snapshot = model.step(0.1, { active: true, reducedMotion: true });
      peak = Math.max(peak, snapshot.lightning);
      expect(snapshot.activity).toBe(1);
    }
    expect(peak).toBe(0);
  });
});
