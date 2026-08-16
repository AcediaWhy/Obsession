import { describe, expect, it } from "vitest";

import {
  CHOIR_CURVES,
  CHOIR_EYES,
  choirCurveToSvgPath,
  createChoirRibbonGeometry,
} from "./geometry";

describe("Black Choir deterministic geometry", () => {
  it("builds exactly 48 curves across three optical depths", () => {
    expect(CHOIR_CURVES).toHaveLength(48);
    expect(new Set(CHOIR_CURVES.map((curve) => curve.depth))).toEqual(new Set([0, 1, 2]));
    expect(CHOIR_EYES).toHaveLength(9);
  });

  it("tessellates each curve into antialiased ribbon triangles", () => {
    const geometry = createChoirRibbonGeometry(48);
    expect(geometry.curveVertexOffsets).toHaveLength(49);
    expect(geometry.curveVertexOffsets[geometry.curveVertexOffsets.length - 1]).toBe(48 * 48 * 6);
    expect(geometry.vertices).toHaveLength(48 * 48 * 6 * geometry.vertexStride);
    expect([...geometry.vertices].every(Number.isFinite)).toBe(true);
  });

  it("shares normalized curves with the SVG fallback", () => {
    const path = choirCurveToSvgPath(CHOIR_CURVES[0]);
    expect(path).toMatch(/^M -?\d+\.\d{2} -?\d+\.\d{2} C /);
    expect(path).not.toContain("NaN");
  });

  it("keeps every engraving ribbon monotonic without self-reversing hooks", () => {
    for (const curve of CHOIR_CURVES) {
      expect(curve.start.x).toBeLessThan(curve.controlA.x);
      expect(curve.controlA.x).toBeLessThan(curve.controlB.x);
      expect(curve.controlB.x).toBeLessThan(curve.end.x);
    }
  });
});
