import { describe, expect, it } from "vitest";

import { normalizeYaniPanelLenses } from "./panelLenses";

describe("Yani Neko glass geometry", () => {
  it("normalizes screen rectangles into bottom-left WebGL space", () => {
    expect(normalizeYaniPanelLenses(
      [{ left: 200, top: 170, width: 400, height: 300, radius: 20 }],
      { left: 100, top: 70, width: 1000, height: 680 },
    )).toEqual([{ x: 0.1, y: 1 - 400 / 680, width: 0.4, height: 300 / 680, radius: 20 / 680 }]);
  });

  it("filters empty panels and never exceeds twelve", () => {
    const panels = Array.from({ length: 20 }, (_, index) => ({ left: index, top: index, width: 10, height: 10 }));
    panels[3].width = 0;
    expect(normalizeYaniPanelLenses(panels, { left: 0, top: 0, width: 100, height: 100 }, 99)).toHaveLength(12);
    expect(normalizeYaniPanelLenses(panels, { left: 0, top: 0, width: 0, height: 100 })).toEqual([]);
  });
});
