import { describe, expect, it } from "vitest";

import {
  CHOIR_COMPOSITE_FRAGMENT_SHADER,
  CHOIR_RIBBON_FRAGMENT_SHADER,
  CHOIR_RIBBON_VERTEX_SHADER,
  CHOIR_WORLD_FRAGMENT_SHADER,
} from "./shaders";

describe("Black Choir shader contracts", () => {
  it("renders one master eye and a nine-eye latent choir without HUD geometry", () => {
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("index < 9");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("masterMask");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("u_body");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("u_pupil_scale");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("glintOrbit");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("counterFiber");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("vec2(0.076, 0.054)");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("no sclera");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).toContain("vec3(0.0006, 0.0007, 0.001)");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).not.toContain("ticks72");
    expect(CHOIR_WORLD_FRAGMENT_SHADER).not.toContain("radialRays");
  });

  it("animates real ribbon vertices and cuts them out of the master void", () => {
    expect(CHOIR_RIBBON_VERTEX_SHADER).toContain("a_normal * tensionWarp");
    expect(CHOIR_RIBBON_VERTEX_SHADER).toContain("u_line_flow");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("masterCut");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("u_lid_open");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("travellingFine");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("depthAlpha");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("fwidth(v_side)");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("filament");
    expect(CHOIR_RIBBON_FRAGMENT_SHADER).toContain("fieldBalance");
  });

  it("caps the composite pass at twelve refractive panel lenses", () => {
    expect(CHOIR_COMPOSITE_FRAGMENT_SHADER).toContain("u_panels[12]");
    expect(CHOIR_COMPOSITE_FRAGMENT_SHADER).toContain("index < 12");
    expect(CHOIR_COMPOSITE_FRAGMENT_SHADER).toContain("refractVector");
  });
});
