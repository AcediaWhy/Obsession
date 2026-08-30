import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { OphanimCatSpriteLab } from "./OphanimCatSpriteLab";

describe("OphanimCatSpriteLab", () => {
  it("uses the compact detail tier at preview size", () => {
    const markup = renderToStaticMarkup(
      <OphanimCatSpriteLab detail="auto" phase="idle" size={104} />,
    );

    expect(markup).toContain("data-ophanim-cat-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="idle"');
  });

  it("opens the guardian state for focused hero rendering", () => {
    const markup = renderToStaticMarkup(
      <OphanimCatSpriteLab detail="hero" phase="focused" size={240} />,
    );

    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-phase="focused"');
  });

  it("maps fault to alarm and respects paused motion", () => {
    const markup = renderToStaticMarkup(
      <OphanimCatSpriteLab paused phase="fault" size={240} />,
    );

    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="paused"');
  });
});
