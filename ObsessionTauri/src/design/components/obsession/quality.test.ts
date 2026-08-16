import { describe, expect, it } from "vitest";

import { obsessionQualityProfile } from "./quality";

describe("obsessionQualityProfile", () => {
  it("targets 60 FPS only on high and scales secondary optics by tier", () => {
    expect(obsessionQualityProfile("high")).toEqual({
      resolutionScale: 1,
      targetFps: 60,
      threadCount: 16,
      caustics: true,
      aberration: 1,
    });
    expect(obsessionQualityProfile("balanced")).toEqual({
      resolutionScale: 0.72,
      targetFps: 30,
      threadCount: 10,
      caustics: true,
      aberration: 0.45,
    });
    expect(obsessionQualityProfile("low")).toEqual({
      resolutionScale: 0.52,
      targetFps: 30,
      threadCount: 6,
      caustics: false,
      aberration: 0,
    });
  });
});
