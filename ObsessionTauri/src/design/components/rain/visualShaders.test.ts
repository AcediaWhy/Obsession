import { describe, expect, it } from "vitest";

import { compositeFrag } from "./compositeShaders";
import { rainFocusLods } from "./focus";
import { rainQualityProfile } from "./quality";

describe("Rain visual shader safeguards", () => {
  it("keeps the wet-glass composite free from animated grain", () => {
    expect(compositeFrag).not.toContain("float grain =");
    // Дизер зависит только от координаты пикселя: иначе он мерцает по кадрам.
    expect(compositeFrag).toContain("float ditherNoise(vec2 fragCoord)");
    expect(compositeFrag).not.toMatch(/ditherNoise\([^)]*u_time/);
  });

  it("defocuses the glass and keeps the drop interior a step sharper", () => {
    // Перевёрнутая модель фокуса (резкий фон, мыло внутри капли) делала капли
    // невидимыми: A/B давал среднюю разницу 0.24/255.
    expect(compositeFrag).toContain("textureLod(u_world, v_uv, u_glassLod + mist * u_mistLod)");
    expect(compositeFrag).toContain("textureLod(u_world, dropUv, u_dropLod)");
    expect(compositeFrag).not.toContain("u_fgLod");
    expect(compositeFrag).not.toContain("u_bgLod");
  });

  it("keeps the tuned RainEffect water constants", () => {
    // Жёсткая кромка маски, смещение рефракции в пикселях, matcap блика и
    // тень под каплей — всё из codrops/RainEffect water.frag.
    expect(compositeFrag).toContain("const float ALPHA_MULTIPLY = 6.0");
    expect(compositeFrag).toContain("const float ALPHA_SUBTRACT = 3.0");
    expect(compositeFrag).toContain("const float MIN_REFRACTION = 150.0");
    expect(compositeFrag).toContain("const float REFRACTION_DELTA = 362.0");
    expect(compositeFrag).toContain("uniform sampler2D u_shine");
    expect(compositeFrag).toContain("float shadowAlpha =");
  });

  it("tone maps the HDR world instead of clipping the bokeh", () => {
    expect(compositeFrag).toContain("const float WHITE_POINT");
    expect(compositeFrag).toContain("color * (1.0 + color / (WHITE_POINT * WHITE_POINT))");
  });
});

describe("rainFocusLods", () => {
  const widths = [320, 640, 960, 1280, 1920, 2560, 3840];

  it("keeps the drop interior sharper than the glass on every tier and size", () => {
    for (const tier of ["high", "balanced", "low"] as const) {
      const { mipDepth } = rainQualityProfile(tier);
      for (const width of widths) {
        const lods = rainFocusLods(width, mipDepth);
        expect(lods.dropLod).toBeLessThan(lods.glassLod);
      }
    }
  });

  it("never asks for a mip level deeper than the chain", () => {
    for (const width of widths) {
      const lods = rainFocusLods(width, 4);
      expect(lods.glassLod).toBeLessThanOrEqual(4);
      expect(lods.scatterLod).toBeLessThanOrEqual(4);
      expect(lods.glassLod + lods.mistLod).toBeLessThanOrEqual(4 + 1e-9);
    }
  });

  it("scales the glass defocus with resolution so blur stays relative", () => {
    const small = rainFocusLods(960, 6);
    const large = rainFocusLods(1920, 6);
    expect(large.glassLod - small.glassLod).toBeCloseTo(1, 5);
  });

  it("survives degenerate input without inverting the focus model", () => {
    const lods = rainFocusLods(0, 0);
    expect(lods.dropLod).toBeLessThanOrEqual(lods.glassLod);
    expect(Number.isFinite(lods.glassLod)).toBe(true);
  });
});
