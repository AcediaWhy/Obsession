import { describe, expect, it } from "vitest";

import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import { sampleObsessionChoirMotion } from "./motion";
import { obsessionChoirSealGeometry, obsessionChoirSealSpinSpeed } from "./sealCanvas";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];

describe("Black Choir eye seal geometry", () => {
  it("rotates slowly while off and accelerates smoothly while active", () => {
    const idle = obsessionChoirSealSpinSpeed(0);
    const halfway = obsessionChoirSealSpinSpeed(0.5);
    const active = obsessionChoirSealSpinSpeed(1);
    expect(idle).toBeGreaterThan(0);
    expect(halfway).toBeGreaterThan(idle);
    expect(halfway).toBeLessThan(active);
    expect(active / idle).toBeGreaterThan(5);
    expect(obsessionChoirSealSpinSpeed(-1)).toBe(idle);
    expect(obsessionChoirSealSpinSpeed(2)).toBe(active);
  });

  it("keeps the nine-lobed aperture within the hero canvas", () => {
    for (const phase of PHASES) {
      const motion = sampleObsessionChoirMotion({ time: 8.4, phase, phaseAge: 0.72 });
      const geometry = obsessionChoirSealGeometry(240, motion);
      expect(Object.values(geometry).every(Number.isFinite)).toBe(true);
      expect(geometry.pupilRadius).toBeLessThan(geometry.apertureRadius);
      expect(geometry.apertureRadius).toBeLessThan(geometry.shoulderRadius);
      expect(geometry.shoulderRadius).toBeLessThan(geometry.outerRadius);
      expect(geometry.outerRadius).toBeLessThan(120);
    }
  });
});
