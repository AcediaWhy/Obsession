import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { SunkenStarFieldLab } from "./SunkenStarFieldLab";

describe("SunkenStarFieldLab", () => {
  it("renders the evening rooftop pixel art field", () => {
    const html = renderToStaticMarkup(<SunkenStarFieldLab />);
    expect(html).toContain("data-sunken-star-field-lab");
    expect(html).toContain('data-renderer="source-art-canvas"');
    expect(html).toContain("sunken-star-field-lab__canvas");
    expect(html).not.toContain("<img");
  });

  it("exposes phase and motion state to the field animation", () => {
    const html = renderToStaticMarkup(<SunkenStarFieldLab paused phase="fault" />);
    expect(html).toContain('data-phase="fault"');
    expect(html).toContain('data-motion="paused"');
  });
});
