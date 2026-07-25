import { describe, expect, it } from "vitest";

import { rainQualityProfile } from "./quality";

describe("rainQualityProfile", () => {
  it("keeps water resolution and drop caps bounded by tier", () => {
    expect(rainQualityProfile("high")).toEqual({
      waterScale: 1,
      worldScale: 1,
      mipDepth: 6,
      maxDrops: 900,
      mistGrid: 160,
    });
    expect(rainQualityProfile("balanced")).toEqual({
      waterScale: 0.75,
      worldScale: 0.75,
      mipDepth: 5,
      maxDrops: 600,
      mistGrid: 112,
    });
    expect(rainQualityProfile("low")).toEqual({
      waterScale: 0.5,
      worldScale: 0.5,
      mipDepth: 4,
      maxDrops: 350,
      mistGrid: 80,
    });
  });

  it("keeps the fg/bg mip references reachable in every tier", () => {
    // Композит целится в мипы-эквиваленты текстур демо (96px нутро капли,
    // 384px стекло); mipDepth каждого тира обязан их покрывать при типичной
    // ширине FBO (1920 * worldScale * waterScale).
    for (const tier of ["high", "balanced", "low"] as const) {
      const q = rainQualityProfile(tier);
      const fboWidth = 1920 * q.waterScale * q.worldScale;
      expect(Math.log2(fboWidth / 96)).toBeLessThanOrEqual(q.mipDepth + 0.5);
    }
  });
});
