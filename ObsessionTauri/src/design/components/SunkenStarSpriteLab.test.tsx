import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { SunkenStarSpriteLab, sunkenStarMoodForPhase } from "./SunkenStarSpriteLab";

describe("SunkenStarSpriteLab", () => {
  it("maps the five visual phases", () => {
    expect(sunkenStarMoodForPhase("idle")).toBe("idle");
    expect(sunkenStarMoodForPhase("engaging")).toBe("busy");
    expect(sunkenStarMoodForPhase("scanning")).toBe("scanning");
    expect(sunkenStarMoodForPhase("focused")).toBe("active");
    expect(sunkenStarMoodForPhase("fault")).toBe("alarm");
  });

  it("renders with valid attributes at preview size", () => {
    const markup = renderToStaticMarkup(
      <SunkenStarSpriteLab detail="auto" phase="idle" size={104} />,
    );

    expect(markup).toContain("data-sunken-star-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-renderer="procedural-canvas"');
    expect(markup).not.toContain("<img");
  });

  it("renders a paused hero alarm state", () => {
    const markup = renderToStaticMarkup(
      <SunkenStarSpriteLab detail="hero" paused phase="fault" size={260} />,
    );

    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="paused"');
    expect(markup).toContain('data-phase="fault"');
  });
});
