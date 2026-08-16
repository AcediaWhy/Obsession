import { describe, expect, it } from "vitest";

import { OBSESSION_FRAGMENT_SHADER, OBSESSION_VERTEX_SHADER } from "./shaders";

describe("Obsession shaders", () => {
  it("keeps phase, screen focus and quality controls explicit", () => {
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_phase");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_phase_age");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform vec2 u_focus");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_threads");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_caustics");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_aberration");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform vec2 u_gaze");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_lid_open");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_pupil_scale");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("(v_uv - u_focus)");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("materialUv");
    expect(OBSESSION_FRAGMENT_SHADER).not.toContain("u_focus + u_pointer");
  });

  it("implements fault diplopia without temporal random grain", () => {
    expect(OBSESSION_FRAGMENT_SHADER).toContain("uniform float u_fault_split");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("splitCenter");
    expect(OBSESSION_FRAGMENT_SHADER).toContain("splitIris");
    expect(OBSESSION_FRAGMENT_SHADER).not.toContain("random(");
    expect(OBSESSION_FRAGMENT_SHADER).not.toContain("hash(");
  });

  it("keeps every smoothstep edge pair in specification order", () => {
    expect(OBSESSION_FRAGMENT_SHADER).toContain("float invertedSmoothstep");
    const pairs = [...OBSESSION_FRAGMENT_SHADER.matchAll(/smoothstep\((-?\d+(?:\.\d+)?),\s*(-?\d+(?:\.\d+)?),/g)];
    for (const [, low, high] of pairs) {
      expect(Number(low), `${low} must be below ${high}`).toBeLessThan(Number(high));
    }
  });

  it("uses the shared fullscreen triangle contract", () => {
    expect(OBSESSION_VERTEX_SHADER).toContain("layout(location = 0) in vec2 a_position");
    expect(OBSESSION_VERTEX_SHADER).toContain("out vec2 v_uv");
  });
});
