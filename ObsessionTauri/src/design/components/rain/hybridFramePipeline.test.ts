import { describe, expect, it, vi } from "vitest";

import { runHybridRainFrame, type HybridRainFramePhases } from "./hybridFramePipeline";

describe("hybrid Rain frame pipeline", () => {
  it("runs input, weather, simulation, upload and both passes in order", () => {
    const order: string[] = [];
    const phases: HybridRainFramePhases = {
      input: vi.fn(() => order.push("input")),
      weather: vi.fn(() => order.push("weather")),
      simulate: vi.fn(() => order.push("simulation")),
      uploadWaterMap: vi.fn(() => order.push("water-map")),
      worldPass: vi.fn(() => order.push("world")),
      compositePass: vi.fn(() => order.push("composite")),
    };

    runHybridRainFrame(phases, 1 / 60);

    expect(order).toEqual(["input", "weather", "simulation", "water-map", "world", "composite"]);
  });
});
