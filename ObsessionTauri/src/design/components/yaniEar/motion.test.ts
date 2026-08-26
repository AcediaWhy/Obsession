import { describe, expect, it } from "vitest";

import { createEarSpringPose, earTarget, stepEarSpring, stepEarSpringPose } from "./motion";

describe("Yani ear motion", () => {
  it("keeps seeded micro-movements deterministic", () => {
    const input = { time: 42.5, mood: "idle", side: -1, pointerX: 0.4, pointerY: -0.2, pointerActive: 1 } as const;
    expect(earTarget(input)).toEqual(earTarget(input));
  });

  it("makes the nearer ear orient more strongly toward a pointer", () => {
    const left = earTarget({ time: 2, mood: "idle", side: -1, pointerX: -0.8, pointerY: 0, pointerActive: 1 });
    const right = earTarget({ time: 2, mood: "idle", side: 1, pointerX: -0.8, pointerY: 0, pointerActive: 1 });
    expect(Math.abs(left.yaw)).toBeGreaterThan(Math.abs(right.yaw));
  });

  it("pins both ears back in alarm", () => {
    const left = earTarget({ time: 2, mood: "alarm", side: -1, pointerX: 0, pointerY: 0, pointerActive: 0 });
    const right = earTarget({ time: 2, mood: "alarm", side: 1, pointerX: 0, pointerY: 0, pointerActive: 0 });
    expect(left.pitch).toBeGreaterThan(0.6);
    expect(right.pitch).toBeGreaterThan(0.6);
    expect(left.splay).toBeLessThan(0);
    expect(right.splay).toBeGreaterThan(0);
  });

  it("clamps dropped frames and never produces NaN", () => {
    const spring = { value: 0, velocity: 0 };
    expect(stepEarSpring(spring, 1, 0)).toEqual(spring);
    expect(stepEarSpring(spring, 1, Number.NaN)).toEqual(spring);
    expect(stepEarSpring(spring, 1, 2)).toEqual(stepEarSpring(spring, 1, 1 / 20));
    const pose = earTarget({ time: 0, mood: "idle", side: -1, pointerX: 0, pointerY: 0, pointerActive: 0 });
    const stepped = stepEarSpringPose(createEarSpringPose(pose), { ...pose, pitch: 0.7 }, 1 / 60);
    for (const springValue of Object.values(stepped)) {
      expect(Number.isFinite(springValue.value)).toBe(true);
      expect(Number.isFinite(springValue.velocity)).toBe(true);
    }
  });
});
