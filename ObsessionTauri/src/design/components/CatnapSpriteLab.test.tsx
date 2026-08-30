import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { CatnapSpriteLab } from "./CatnapSpriteLab";

describe("CatnapSpriteLab", () => {
  it("renders the idle scene without scaling artifacts", () => {
    const markup = renderToStaticMarkup(<CatnapSpriteLab phase="idle" size={240} />);

    expect(markup).toContain("catnap-sprite-lab");
    expect(markup).toContain('data-mood="idle"');
    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-motion="running"');
    expect(markup).toContain('viewBox="0 0 52 52"');
  });

  it("switches to awake mode when focused", () => {
    const markup = renderToStaticMarkup(<CatnapSpriteLab phase="focused" size={104} />);

    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-detail="base"');
  });

  it("handles alarm state with high priority", () => {
    const markup = renderToStaticMarkup(<CatnapSpriteLab phase="fault" size={240} />);

    expect(markup).toContain('data-mood="alarm"');
  });
});
