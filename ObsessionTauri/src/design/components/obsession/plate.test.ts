import { describe, expect, it } from "vitest";

import { createObsessionPlate } from "./plate";

describe("createObsessionPlate", () => {
  it("is deterministic and produces paired opaque color/normal maps", () => {
    const first = createObsessionPlate(24);
    const second = createObsessionPlate(24);
    expect(first.size).toBe(24);
    expect(first.color).toEqual(second.color);
    expect(first.normal).toEqual(second.normal);
    expect(first.color).toHaveLength(24 * 24 * 4);
    expect(first.normal).toHaveLength(24 * 24 * 4);
    for (let index = 3; index < first.color.length; index += 4) {
      expect(first.color[index]).toBe(255);
      expect(first.normal[index]).toBe(255);
    }
  });
});
