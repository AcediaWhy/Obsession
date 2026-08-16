import { describe, expect, it } from "vitest";

import { sampleObsessionEyeMotion } from "./eyeMotion";

describe("sampleObsessionEyeMotion", () => {
  it("is deterministic and keeps every channel bounded", () => {
    const input = { time: 17.42, phase: "idle" as const, phaseAge: 8.2, pointerX: 0.7, pointerY: -0.4 };
    const first = sampleObsessionEyeMotion(input);
    expect(sampleObsessionEyeMotion(input)).toEqual(first);
    expect(first.gazeX).toBeGreaterThanOrEqual(-1);
    expect(first.gazeX).toBeLessThanOrEqual(1);
    expect(first.gazeY).toBeGreaterThanOrEqual(-1);
    expect(first.gazeY).toBeLessThanOrEqual(1);
    expect(first.lidOpen).toBeGreaterThanOrEqual(0.04);
    expect(first.lidOpen).toBeLessThanOrEqual(1);
    expect(first.pupilScale).toBeGreaterThanOrEqual(0.55);
    expect(first.pupilScale).toBeLessThanOrEqual(1.35);
  });

  it("contains visible idle saccades, blinks and a rare direct fixation", () => {
    const samples = Array.from({ length: 1200 }, (_, index) =>
      sampleObsessionEyeMotion({ time: index * 0.05, phase: "idle", phaseAge: index * 0.05 }),
    );
    expect(Math.min(...samples.map((sample) => sample.lidOpen))).toBeLessThan(0.2);
    expect(Math.max(...samples.map((sample) => sample.fixation))).toBeGreaterThan(0.9);
    expect(Math.max(...samples.map((sample) => Math.abs(sample.gazeX)))).toBeGreaterThan(0.35);
  });

  it("gives each runtime phase a distinct cinematic posture", () => {
    const engaging = sampleObsessionEyeMotion({ time: 10, phase: "engaging", phaseAge: 0.2 });
    const scanning = sampleObsessionEyeMotion({ time: 10, phase: "scanning", phaseAge: 2 });
    const focused = sampleObsessionEyeMotion({ time: 10, phase: "focused", phaseAge: 2 });
    const fault = sampleObsessionEyeMotion({ time: 10, phase: "fault", phaseAge: 2 });

    expect(engaging.lidOpen).toBeLessThan(0.5);
    expect(scanning.bodyTension).toBeGreaterThan(focused.bodyTension);
    expect(Math.abs(scanning.gazeX)).toBeGreaterThan(Math.abs(focused.gazeX));
    expect(fault.faultSplit).toBeGreaterThan(0.4);
  });
});
