import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AuroraCatSpriteLab, auroraCatMoodForPhase } from "./AuroraCatSpriteLab";

describe("AuroraCatSpriteLab", () => {
  it("maps protection phases onto aurora moods", () => {
    expect(auroraCatMoodForPhase("idle")).toBe("idle");
    expect(auroraCatMoodForPhase("engaging")).toBe("busy");
    expect(auroraCatMoodForPhase("scanning")).toBe("scanning");
    expect(auroraCatMoodForPhase("focused")).toBe("active");
    expect(auroraCatMoodForPhase("fault")).toBe("alarm");
  });

  it("renders an edge-free hero cat with a connected aurora tail", () => {
    const markup = renderToStaticMarkup(<AuroraCatSpriteLab phase="idle" paused size={240} />);

    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain("aurora-cat-sprite-lab__tail-ribbon");
    expect(markup).toContain("aurora-cat-sprite-lab__guiding-star");
    expect(markup).not.toContain("scene-bg");
  });

  it("uses the compact grid for a real theme preview", () => {
    const markup = renderToStaticMarkup(<AuroraCatSpriteLab phase="focused" size={104} />);

    expect(markup).toContain('viewBox="0 0 52 52"');
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="active"');
  });
});
