import { describe, expect, it } from "vitest";

import { RainCoreModel } from "./coreModel";

function advance(model: RainCoreModel, seconds: number, fps: number, active: boolean, busy = false) {
  const frames = Math.round(seconds * fps);
  const dt = seconds / frames;
  let snapshot = model.step(0, { active, busy, reducedMotion: false });
  for (let frame = 0; frame < frames; frame += 1) {
    snapshot = model.step(dt, { active, busy, reducedMotion: false });
  }
  return snapshot;
}

describe("RainCoreModel", () => {
  it("mounts in ON without replaying the transition drop", () => {
    const snapshot = new RainCoreModel(true).step(1 / 60, {
      active: true,
      busy: false,
      reducedMotion: false,
    });
    expect(snapshot).toMatchObject({ label: "ON", light: 1, water: 0.45, transitionDrop: null });
  });

  it("emits one transition drop only on OFF to ON", () => {
    const model = new RainCoreModel(false);
    model.step(0.1, { active: false, busy: true, reducedMotion: false });
    const started = model.step(0.1, { active: true, busy: false, reducedMotion: false });
    expect(started.transitionDrop).not.toBeNull();
    const settled = advance(model, 2.1, 60, true);
    expect(settled.transitionDrop).toBeNull();
    expect(model.step(0.1, { active: true, busy: false, reducedMotion: false }).transitionDrop).toBeNull();
  });

  it("keeps light and water transitions stable across refresh rates", () => {
    const at60 = advance(new RainCoreModel(false), 1.5, 60, true);
    const at165 = advance(new RainCoreModel(false), 1.5, 165, true);
    expect(at60.light).toBeCloseTo(at165.light, 5);
    expect(at60.water).toBeCloseTo(at165.water, 5);
  });

  it("latches the direction of a busy transition", () => {
    const stopping = new RainCoreModel(true);
    const busy = stopping.step(0.5, { active: true, busy: true, reducedMotion: false });
    const backendFlipped = stopping.step(0.5, { active: false, busy: true, reducedMotion: false });
    expect(busy.light).toBeGreaterThan(backendFlipped.light);
    expect(backendFlipped.label).toBe("···");
  });

  it("snaps to a static poster and suppresses transition events for reduced motion", () => {
    const snapshot = new RainCoreModel(false).step(1 / 60, {
      active: true,
      busy: false,
      reducedMotion: true,
    });
    expect(snapshot).toEqual({ light: 1, water: 0.45, label: "ON", transitionDrop: null });
  });
});

