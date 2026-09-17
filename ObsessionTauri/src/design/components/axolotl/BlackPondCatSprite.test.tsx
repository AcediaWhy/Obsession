import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { BlackPondCatSprite } from "./BlackPondCatSprite";

describe("BlackPondCatSprite idle-only replacement", () => {
  it("uses the approved layered renderer only for running idle", () => {
    const html = renderToStaticMarkup(<BlackPondCatSprite phase="idle" size={240} />);
    expect(html).toContain('data-idle-renderer="layered"');
    expect(html).toContain("idle-256-still.webp");
    expect(html).not.toContain("idle-256.webp");
  });

  for (const phase of ["engaging", "scanning", "focused", "fault"] as const) {
    for (const size of [104, 240]) {
      for (const paused of [false, true]) {
        it(`preserves ${phase} at ${size}px, paused=${paused}`, () => {
          const html = renderToStaticMarkup(<BlackPondCatSprite phase={phase} size={size} paused={paused} />);
          expect(html).toContain(`${phase}-${size <= 128 ? 128 : 256}${paused ? "-still" : ""}.webp`);
          expect(html).not.toContain("data-idle-renderer");
          expect(html).toContain(`<img`);
        });
      }
    }
  }

  it("keeps paused idle as a still without mounting the animation", () => {
    const html = renderToStaticMarkup(<BlackPondCatSprite phase="idle" size={104} paused />);
    expect(html).toContain("idle-128-still.webp");
    expect(html).toContain('data-motion="paused"');
    expect(html).not.toContain("data-idle-renderer");
  });
});
