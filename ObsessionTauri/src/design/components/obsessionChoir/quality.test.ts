import { describe, expect, it } from "vitest";

import { obsessionChoirQuality } from "./quality";

describe("Black Choir quality policy", () => {
  it("targets 60 FPS only on high and progressively removes secondary work", () => {
    const high = obsessionChoirQuality("high");
    const balanced = obsessionChoirQuality("balanced");
    const low = obsessionChoirQuality("low");
    expect(high.targetFps).toBe(60);
    expect(balanced.targetFps).toBe(30);
    expect(low.targetFps).toBe(30);
    expect(high.curveCount).toBe(48);
    expect(high.curveSegments).toBe(144);
    expect(high.resolutionScale).toBeGreaterThan(1);
    expect(balanced.curveCount).toBeLessThan(high.curveCount);
    expect(balanced.curveSegments).toBe(96);
    expect(balanced.resolutionScale).toBe(1);
    expect(low.curveCount).toBeLessThan(balanced.curveCount);
    expect(low.curveSegments).toBe(64);
    expect(low.resolutionScale).toBeGreaterThanOrEqual(0.8);
    expect(low.caustics).toBe(false);
    expect(low.aberration).toBe(0);
    expect(low.panelCount).toBe(6);
  });
});
