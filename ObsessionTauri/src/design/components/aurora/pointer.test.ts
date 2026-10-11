import { describe, expect, it } from "vitest";
import { AuroraPointerMotion } from "./pointer";

describe("Aurora pointer inertia", () => {
  it("enters at the actual cursor and eases the influence in", () => {
    const motion = new AuroraPointerMotion();
    motion.move(0.8, 0.2);
    expect(motion.update(0)).toEqual({ x: 0.8, y: 0.2, strength: 0 });
    const first = motion.update(1 / 30);
    expect(first.strength).toBeGreaterThan(0);
    expect(first.strength).toBeLessThan(0.02);
  });
  it("has the same trajectory at 30 and 60 fps", () => {
    const run = (fps: number) => {
      const motion = new AuroraPointerMotion();
      motion.move(0.1, 0.3); motion.move(0.9, 0.8);
      let frame = motion.update(0);
      for (let i = 0; i < fps; i++) frame = motion.update(1 / fps);
      return frame;
    };
    const a = run(30), b = run(60);
    expect(a.x).toBeCloseTo(b.x, 8);
    expect(a.y).toBeCloseTo(b.y, 8);
    expect(a.strength).toBeCloseTo(b.strength, 8);
  });
  it("releases gradually on leave and after inactivity", () => {
    const motion = new AuroraPointerMotion();
    motion.move(0.8, 0.4);
    const active = motion.update(2);
    motion.leave();
    const leaving = motion.update(1 / 30);
    expect(leaving.strength).toBeLessThan(active.strength);
    expect(leaving.strength).toBeGreaterThan(0.95);
    expect(motion.update(4).strength).toBeLessThan(0.001);
    motion.move(0.8, 0.4);
    for (let i = 0; i < 600; i++) motion.update(1 / 30);
    expect(motion.update(0).strength).toBeLessThan(0.001);
  });
  it("clears retained motion for reduced motion and a new entry", () => {
    const motion = new AuroraPointerMotion();
    motion.move(-2, 5); motion.update(1);
    motion.reset();
    expect(motion.update(0)).toEqual({ x: 0.5, y: 0.5, strength: 0 });
    motion.move(0.2, 0.1);
    expect(motion.update(0)).toEqual({ x: 0.2, y: 0.1, strength: 0 });
  });
});
