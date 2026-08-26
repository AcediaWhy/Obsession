import { describe, expect, it } from "vitest";

import { sampleYaniMotion, smoothYaniMotion, yaniEventPulse } from "./motion";
import type { YaniMood } from "./types";

const MOODS: readonly YaniMood[] = ["idle", "busy", "scanning", "active", "alarm"];

describe("Yani Neko motion model", () => {
  it("is deterministic and finite for every mood", () => {
    for (const mood of MOODS) {
      const input = { time: 42.75, mood, moodAge: 1.4, hover: 0.8, pointerX: -0.4, pointerY: 0.2 };
      const first = sampleYaniMotion(input);
      expect(sampleYaniMotion(input)).toEqual(first);
      for (const value of Object.values(first)) expect(Number.isFinite(value)).toBe(true);
      expect(first.gazeX).toBeGreaterThanOrEqual(-1);
      expect(first.gazeX).toBeLessThanOrEqual(1);
      expect(first.gazeY).toBeGreaterThanOrEqual(-1);
      expect(first.gazeY).toBeLessThanOrEqual(1);
    }
  });

  it("keeps seeded micro-events stable", () => {
    const samples = Array.from({ length: 400 }, (_, index) => yaniEventPulse(index / 60, 5.35, 1.15, 0.16, 17));
    expect(samples).toEqual(Array.from({ length: 400 }, (_, index) =>
      yaniEventPulse(index / 60, 5.35, 1.15, 0.16, 17),
    ));
    expect(samples.some((value) => value > 0.8)).toBe(true);
  });

  it("handles zero dt, dropped frames and invalid resume input without jumps or NaN", () => {
    const idle = sampleYaniMotion({ time: 4, mood: "idle", moodAge: 4 });
    const alarm = sampleYaniMotion({ time: 4, mood: "alarm", moodAge: 0 });
    expect(smoothYaniMotion(idle, alarm, 0)).toEqual(idle);
    expect(smoothYaniMotion(idle, alarm, Number.NaN)).toEqual(idle);

    const dropped = smoothYaniMotion(idle, alarm, 2);
    const normal = smoothYaniMotion(idle, alarm, 0.25);
    expect(dropped).toEqual(normal);
    for (const key of Object.keys(idle) as (keyof typeof idle)[]) {
      expect(Number.isFinite(dropped[key])).toBe(true);
      expect(dropped[key]).toBeGreaterThanOrEqual(Math.min(idle[key], alarm[key]));
      expect(dropped[key]).toBeLessThanOrEqual(Math.max(idle[key], alarm[key]));
    }
  });

  it("gives each system state its defining expression", () => {
    const idle = sampleYaniMotion({ time: 2, mood: "idle", moodAge: 2 });
    const busy = sampleYaniMotion({ time: 2, mood: "busy", moodAge: 0.4 });
    const scanning = sampleYaniMotion({ time: 2, mood: "scanning", moodAge: 1 });
    const active = sampleYaniMotion({ time: 2, mood: "active", moodAge: 1 });
    const alarm = sampleYaniMotion({ time: 2, mood: "alarm", moodAge: 1 });
    expect(busy.ember).toBeGreaterThan(idle.ember);
    expect(scanning.eyeOpen).toBeGreaterThan(active.eyeOpen);
    expect(active.smoke).toBeGreaterThan(idle.smoke);
    expect(alarm.grimace).toBeGreaterThan(0.9);
    expect(alarm.earFlat).toBeGreaterThan(scanning.earFlat);
    expect(alarm.ember).toBeLessThan(idle.ember);
  });
});
