import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { YaniCatSpriteLab } from "./YaniCatSpriteLab";

describe("YaniCatSpriteLab", () => {
  it("renders the new cat silhouette in the idle state", () => {
    const markup = renderToStaticMarkup(<YaniCatSpriteLab phase="idle" />);

    expect(markup).toContain('data-mood="idle"');
    expect(markup).toContain("yani-cat-sprite-lab__inner-ear--left");
    expect(markup).toContain("yani-cat-sprite-lab__tail-tip");
    expect(markup).toContain("yani-cat-sprite-lab__whiskers");
    expect(markup).not.toContain("yani-cat-sprite-lab__cigarette");
  });

  it("maps runtime phases to cat moods and respects paused motion", () => {
    const active = renderToStaticMarkup(<YaniCatSpriteLab phase="focused" />);
    const alarm = renderToStaticMarkup(<YaniCatSpriteLab phase="fault" paused />);

    expect(active).toContain('data-mood="active"');
    expect(alarm).toContain('data-mood="alarm"');
    expect(alarm).toContain('data-motion="still"');
  });

  it("can render as a standalone theme core without the device shell", () => {
    const markup = renderToStaticMarkup(<YaniCatSpriteLab phase="idle" standalone />);

    expect(markup).toContain('data-presentation="standalone"');
    expect(markup).toContain("translate(40 28) scale(8)");
  });
});
