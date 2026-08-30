import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { MidnightSpriteLab } from "./MidnightSpriteLab";

describe("MidnightSpriteLab", () => {
  it("renders with crisp edges and accessible label", () => {
    const markup = renderToStaticMarkup(<MidnightSpriteLab phase="idle" size={240} />);

    expect(markup).toContain("data-midnight-sprite-lab");
    expect(markup).toContain('shape-rendering="crispEdges"');
    expect(markup).toContain('data-mood="idle"');
  });

  it("handles active focused state", () => {
    const markup = renderToStaticMarkup(<MidnightSpriteLab phase="focused" size={240} />);

    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-phase="focused"');
  });

  it("resolves detail correctly", () => {
    const heroMarkup = renderToStaticMarkup(<MidnightSpriteLab detail="auto" size={240} />);
    expect(heroMarkup).toContain('data-detail="hero"');

    const baseMarkup = renderToStaticMarkup(<MidnightSpriteLab detail="auto" size={104} />);
    expect(baseMarkup).toContain('data-detail="base"');
  });
});
