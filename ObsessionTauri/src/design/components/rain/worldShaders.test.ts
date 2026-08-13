import { describe, expect, it } from "vitest";

import { rainPassVert, worldPlateFrag } from "./worldShaders";

describe("Rain world shader", () => {
  it("builds the world from the baked plate and its emission map", () => {
    expect(worldPlateFrag).toContain("uniform sampler2D u_plate");
    expect(worldPlateFrag).toContain("uniform sampler2D u_emission");
    // Cover-fit: плита заполняет кадр без искажения пропорций.
    expect(worldPlateFrag).toContain("uniform float u_plateAspect");
    expect(worldPlateFrag).toContain("vec2 plateUv(vec2 uv, vec2 shift)");
  });

  it("has no analytic silhouette masks left", () => {
    // Маски-силуэты (горы/лес/камыш) читались как набор геометрических фигур;
    // структуру кадра теперь даёт реальная подложка.
    expect(worldPlateFrag).not.toContain("forestMask");
    expect(worldPlateFrag).not.toContain("reedMask");
    expect(worldPlateFrag).not.toContain("mountainMask");
  });

  it("lifts light sources above one so drops can collect bokeh", () => {
    expect(worldPlateFrag).toContain("const float EMISSION_GAIN");
    expect(worldPlateFrag).toContain("const float HALO_GAIN");
    // Ореол берётся из мипов карты эмиссии.
    expect(worldPlateFrag).toContain("textureLod(u_emission, emissionUv, 4.0)");
  });

  it("writes linear light for the composite to tone map", () => {
    expect(worldPlateFrag).toContain("vec3 toLinear(vec3 color)");
    expect(worldPlateFrag).not.toContain("1.0 / 2.2");
  });

  it("keeps the shared fullscreen-triangle vertex shader", () => {
    expect(rainPassVert).toContain("layout(location = 0) in vec2 a_position");
    expect(rainPassVert).toContain("out vec2 v_uv");
  });
});
