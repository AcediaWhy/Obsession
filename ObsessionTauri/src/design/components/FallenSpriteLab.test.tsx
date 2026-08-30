import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { FallenSpriteLab } from "./FallenSpriteLab";

describe("FallenSpriteLab", () => {
  it("renders base detail tier cleanly on compact sizes", () => {
    const markup = renderToStaticMarkup(
      <FallenSpriteLab detail="auto" phase="idle" size={104} />,
    );

    expect(markup).toContain("data-fallen-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="idle"');
  });

  it("renders hero detail tier and maps active phase to active mood", () => {
    const markup = renderToStaticMarkup(
      <FallenSpriteLab detail="hero" phase="focused" size={240} />,
    );

    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-phase="focused"');
  });

  it("maps fault phase to alarm mood with paused state respected", () => {
    const markup = renderToStaticMarkup(
      <FallenSpriteLab phase="fault" paused size={240} />,
    );

    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="paused"');
  });
});
