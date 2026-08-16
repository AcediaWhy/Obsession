import { describe, expect, it } from "vitest";

import { normalizePanelLenses } from "./panelLenses";

describe("Black Choir panel lenses", () => {
  it("normalizes top-left DOM rectangles into bottom-left shader space", () => {
    expect(normalizePanelLenses(
      [{ left: 100, top: 50, width: 200, height: 100, radius: 20 }],
      { left: 0, top: 0, width: 1000, height: 500 },
    )).toEqual([{ x: 0.1, y: 0.7, width: 0.2, height: 0.2, radius: 0.04 }]);
  });

  it("drops empty panels and caps the shader payload at twelve lenses", () => {
    const panels = Array.from({ length: 18 }, (_, index) => ({
      left: index * 10,
      top: 20,
      width: index === 2 ? 0 : 80,
      height: 60,
      radius: 12,
    }));
    expect(normalizePanelLenses(panels, { left: 0, top: 0, width: 1000, height: 680 }, 20)).toHaveLength(12);
    expect(normalizePanelLenses(panels, { left: 0, top: 0, width: 0, height: 680 })).toEqual([]);
  });
});
