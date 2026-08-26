import { describe, expect, it } from "vitest";

import {
  YANI_COMPOSITE_FRAGMENT_SHADER,
  YANI_FULLSCREEN_VERTEX_SHADER,
  YANI_SMOKE_FRAGMENT_SHADER,
  YANI_WORLD_FRAGMENT_SHADER,
} from "./shaders";

describe("Yani Neko shader contracts", () => {
  it("uses WebGL2 and a shared fullscreen varying", () => {
    for (const source of [YANI_FULLSCREEN_VERTEX_SHADER, YANI_SMOKE_FRAGMENT_SHADER, YANI_WORLD_FRAGMENT_SHADER, YANI_COMPOSITE_FRAGMENT_SHADER]) {
      expect(source).toContain("#version 300 es");
      expect(source).toContain("v_uv");
    }
  });

  it("declares all three pass inputs and bounded panel lenses", () => {
    for (const uniformName of ["u_previous", "u_pointer", "u_dt", "u_reset", "u_octaves"]) {
      expect(YANI_SMOKE_FRAGMENT_SHADER).toContain(`uniform`);
      expect(YANI_SMOKE_FRAGMENT_SHADER).toContain(uniformName);
    }
    for (const uniformName of ["u_scene_shift", "u_mood", "u_dust_count"]) {
      expect(YANI_WORLD_FRAGMENT_SHADER).toContain(uniformName);
    }
    for (const uniformName of ["u_world", "u_smoke", "u_panel_count", "u_panels[12]", "u_panel_radii[12]"]) {
      expect(YANI_COMPOSITE_FRAGMENT_SHADER).toContain(uniformName);
    }
    expect(YANI_SMOKE_FRAGMENT_SHADER).toContain("curlVelocity");
    expect(YANI_COMPOSITE_FRAGMENT_SHADER).toContain("roundedPanel");
  });
});
