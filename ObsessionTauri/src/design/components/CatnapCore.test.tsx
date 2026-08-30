import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { CatnapCore } from "./CatnapCore";

describe("CatnapCore", () => {
  it("renders the compact pixel scene without a nested preview button", () => {
    const markup = renderToStaticMarkup(
      <CatnapCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-catnap-core");
    expect(markup).toContain("catnap-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <CatnapCore active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="running"');
  });

  it("maps active state to focused / active mood", () => {
    const markup = renderToStaticMarkup(
      <CatnapCore active={true} onClick={() => {}} />,
    );

    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-phase="focused"');
  });
});
