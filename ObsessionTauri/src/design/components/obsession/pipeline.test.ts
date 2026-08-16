import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  programDispose: vi.fn(),
  set1f: vi.fn(),
  set2f: vi.fn(),
}));

vi.mock("../rain/gl2", () => ({
  getContext2: (canvas: HTMLCanvasElement) =>
    (canvas as unknown as { __gl: WebGL2RenderingContext }).__gl,
  createProgram2: () => ({}) as WebGLProgram,
  Gl2Program: class {
    use() {}
    set1f(name: string, value: number) {
      mocks.set1f(name, value);
    }
    set2f(name: string, x: number, y: number) {
      mocks.set2f(name, x, y);
    }
    set1i() {}
    dispose() {
      mocks.programDispose();
    }
  },
}));

import { ObsessionPipeline } from "./pipeline";
import { obsessionQualityProfile } from "./quality";

function fakeGl() {
  let id = 0;
  return {
    ARRAY_BUFFER: 1,
    STATIC_DRAW: 2,
    FLOAT: 3,
    TEXTURE_2D: 4,
    TEXTURE_WRAP_S: 5,
    TEXTURE_WRAP_T: 6,
    REPEAT: 7,
    TEXTURE_MIN_FILTER: 8,
    TEXTURE_MAG_FILTER: 9,
    LINEAR_MIPMAP_LINEAR: 10,
    LINEAR: 11,
    RGBA8: 12,
    RGBA: 13,
    UNSIGNED_BYTE: 14,
    BLEND: 15,
    TEXTURE0: 16,
    TEXTURE1: 17,
    TRIANGLES: 18,
    createVertexArray: vi.fn(() => ({ id: ++id })),
    createBuffer: vi.fn(() => ({ id: ++id })),
    createTexture: vi.fn(() => ({ id: ++id })),
    bindVertexArray: vi.fn(),
    bindBuffer: vi.fn(),
    bufferData: vi.fn(),
    enableVertexAttribArray: vi.fn(),
    vertexAttribPointer: vi.fn(),
    bindTexture: vi.fn(),
    texParameteri: vi.fn(),
    texImage2D: vi.fn(),
    generateMipmap: vi.fn(),
    viewport: vi.fn(),
    disable: vi.fn(),
    activeTexture: vi.fn(),
    drawArrays: vi.fn(),
    deleteTexture: vi.fn(),
    deleteBuffer: vi.fn(),
    deleteVertexArray: vi.fn(),
    isContextLost: vi.fn(() => false),
  };
}

describe("ObsessionPipeline lifecycle", () => {
  beforeEach(() => {
    mocks.programDispose.mockClear();
    mocks.set1f.mockClear();
    mocks.set2f.mockClear();
  });

  it("renders once and releases both optical textures plus geometry", () => {
    const gl = fakeGl();
    const canvas = { __gl: gl } as unknown as HTMLCanvasElement;
    const pipeline = new ObsessionPipeline(canvas, obsessionQualityProfile("high"));
    pipeline.render(800, 600, {
      time: 0,
      phase: 0,
      phaseAge: 0,
      focusX: 0.79,
      focusY: 0.76,
      pointerX: 0,
      pointerY: 0,
      capture: 1,
      gazeX: 0.4,
      gazeY: -0.2,
      lidOpen: 0.9,
      pupilScale: 1.1,
      bodyTension: 0.5,
      irisRotation: 0.4,
      highlightPhase: 0.2,
      fixation: 0,
      faultSplit: 0,
    });

    expect(gl.drawArrays).toHaveBeenCalledOnce();
    expect(mocks.set2f).toHaveBeenCalledWith("u_gaze", 0.4, -0.2);
    expect(mocks.set1f).toHaveBeenCalledWith("u_lid_open", 0.9);
    expect(mocks.set1f).toHaveBeenCalledWith("u_pupil_scale", 1.1);
    expect(mocks.set1f).toHaveBeenCalledWith("u_fault_split", 0);
    pipeline.destroy();
    pipeline.destroy();
    expect(mocks.programDispose).toHaveBeenCalledOnce();
    expect(gl.deleteTexture).toHaveBeenCalledTimes(2);
    expect(gl.deleteBuffer).toHaveBeenCalledOnce();
    expect(gl.deleteVertexArray).toHaveBeenCalledOnce();
  });

  it("does not call invalid GL cleanup after context loss", () => {
    const gl = fakeGl();
    const canvas = { __gl: gl } as unknown as HTMLCanvasElement;
    const pipeline = new ObsessionPipeline(canvas, obsessionQualityProfile("low"));
    pipeline.abandonAfterContextLoss();
    pipeline.destroy();
    expect(gl.deleteTexture).not.toHaveBeenCalled();
    expect(mocks.programDispose).not.toHaveBeenCalled();
  });
});
