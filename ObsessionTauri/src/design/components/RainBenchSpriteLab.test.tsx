import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { RainBenchSpriteLab, rainBenchMoodForPhase } from "./RainBenchSpriteLab";

describe("RainBenchSpriteLab", () => {
  it("maps shared visual phases to the five bench moods", () => {
    expect(rainBenchMoodForPhase("idle")).toBe("idle");
    expect(rainBenchMoodForPhase("engaging")).toBe("busy");
    expect(rainBenchMoodForPhase("scanning")).toBe("scanning");
    expect(rainBenchMoodForPhase("focused")).toBe("active");
    expect(rainBenchMoodForPhase("fault")).toBe("alarm");
  });

  it("renders a decorative crisp-edge scene without an interactive wrapper", () => {
    const markup = renderToStaticMarkup(
      <RainBenchSpriteLab phase="focused" paused size={104} />,
    );

    expect(markup).toContain("data-rain-bench-sprite");
    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-motion="still"');
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('viewBox="0 0 52 52"');
    expect(markup).toContain('shape-rendering="crispEdges"');
    expect(markup).toContain("rain-bench-sprite-lab__bench");
    expect(markup).toContain("rain-bench-sprite-lab__cat");
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("Gradient");
  });

  it("enables the fine-pixel layer for hero-sized renders", () => {
    const markup = renderToStaticMarkup(
      <RainBenchSpriteLab phase="idle" size={240} />,
    );

    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('viewBox="0 0 120 120"');
    expect(markup).toContain("rain-bench-sprite-lab__bench-nail");
    expect(markup).toContain("rain-bench-sprite-lab__cat-head");
  });
});
