import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { FallenCore } from "./FallenCore";

describe("FallenCore", () => {
  it("renders the compact Temmie scene without a nested preview button", () => {
    const markup = renderToStaticMarkup(
      <FallenCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-fallen-core");
    expect(markup).toContain("fallen-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <FallenCore active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-phase="fault"');
  });

  it("maps active state to focused motion", () => {
    const markup = renderToStaticMarkup(
      <FallenCore active onClick={() => {}} />,
    );

    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-phase="focused"');
    expect(markup).toContain('data-motion="running"');
  });
});
