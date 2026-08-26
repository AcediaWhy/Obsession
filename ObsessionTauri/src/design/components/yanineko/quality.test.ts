import { describe, expect, it } from "vitest";

import { yaniQuality } from "./quality";

describe("Yani Neko adaptive quality", () => {
  it("decreases every expensive budget monotonically", () => {
    const high = yaniQuality("high");
    const balanced = yaniQuality("balanced");
    const low = yaniQuality("low");
    for (const key of ["maxFps", "worldScale", "smokeScale", "noiseOctaves", "panelCount", "dustCount"] as const) {
      expect(high[key]).toBeGreaterThan(balanced[key]);
      expect(balanced[key]).toBeGreaterThan(low[key]);
    }
    expect(high).toMatchObject({ maxFps: 180, worldScale: 0.9, smokeScale: 0.5, noiseOctaves: 5, panelCount: 12 });
    expect(low).toMatchObject({ maxFps: 60, worldScale: 0.6, smokeScale: 0.28, noiseOctaves: 3, panelCount: 6 });
  });
});
