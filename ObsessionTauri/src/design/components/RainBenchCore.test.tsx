import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { RainBenchCore } from "./RainBenchCore";

describe("RainBenchCore", () => {
  it("renders the compact pixel scene without a nested preview button", () => {
    const markup = renderToStaticMarkup(
      <RainBenchCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-rain-bench-core");
    expect(markup).toContain("rain-umbrella-sprite-lab");
    expect(markup).not.toContain("rain-bench-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <RainBenchCore active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="running"');
  });
});
