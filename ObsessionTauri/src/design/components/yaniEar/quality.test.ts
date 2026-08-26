import { describe, expect, it } from "vitest";

import { earQualityProfile } from "./quality";

describe("Yani ear quality", () => {
  it("reduces every expensive budget monotonically", () => {
    const high = earQualityProfile("high");
    const balanced = earQualityProfile("balanced");
    const low = earQualityProfile("low");
    for (const key of ["pixelRatio", "shellCount", "hairRatio", "textureSize", "shadowSize"] as const) {
      expect(high[key]).toBeGreaterThan(balanced[key]);
      expect(balanced[key]).toBeGreaterThan(low[key]);
    }
  });
});
