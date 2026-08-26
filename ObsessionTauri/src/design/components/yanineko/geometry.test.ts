import { describe, expect, it } from "vitest";

import { yaniCoreGeometry, yaniGeometryFits } from "./geometry";

describe("Yani Neko core geometry", () => {
  it.each([104, 220, 240])("keeps ears, rings, face and cigarette inside %ipx", (size) => {
    expect(yaniGeometryFits(size)).toBe(true);
    const geometry = yaniCoreGeometry(size);
    expect(geometry.leftEarTip.y).toBeGreaterThanOrEqual(0);
    expect(geometry.rightEarTip.y).toBeGreaterThanOrEqual(0);
    expect(geometry.cigaretteEnd.x).toBeLessThanOrEqual(size);
    expect(geometry.headBottom).toBeLessThanOrEqual(size);
  });

  it("sanitizes unusable sizes", () => {
    expect(yaniCoreGeometry(Number.NaN).size).toBe(1);
    expect(yaniCoreGeometry(-10).size).toBe(1);
  });
});
