import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ createProgram: vi.fn() }));

vi.mock("../rain/gl2", () => ({
  getContext2: (canvas: HTMLCanvasElement) =>
    (canvas as unknown as { __gl: WebGL2RenderingContext }).__gl,
  createProgram2: () => mocks.createProgram(),
}));

import { YaniNekoPipeline } from "./pipeline";
import { yaniQuality } from "./quality";
import type { YaniFieldFrame } from "./types";

function fakeGl() {
  let id = 0;
  return {
    ARRAY_BUFFER: 1,
    STATIC_DRAW: 2,
    FLOAT: 3,
    TEXTURE_2D: 4,
    TEXTURE_WRAP_S: 5,
    TEXTURE_WRAP_T: 6,
    CLAMP_TO_EDGE: 7,
    TEXTURE_MIN_FILTER: 8,
    TEXTURE_MAG_FILTER: 9,
    LINEAR: 10,
    FRAMEBUFFER: 11,
    COLOR_ATTACHMENT0: 12,
    RGBA8: 13,
    RGBA: 14,
    UNSIGNED_BYTE: 15,
    BLEND: 16,
    TRIANGLES: 17,
    TEXTURE0: 18,
    TEXTURE1: 19,
    COLOR_BUFFER_BIT: 20,
    createVertexArray: vi.fn(() => ({ id: ++id })),
    createBuffer: vi.fn(() => ({ id: ++id })),
    createFramebuffer: vi.fn(() => ({ id: ++id })),
    createTexture: vi.fn(() => ({ id: ++id })),
    bindVertexArray: vi.fn(), bindBuffer: vi.fn(), bufferData: vi.fn(),
    enableVertexAttribArray: vi.fn(), vertexAttribPointer: vi.fn(),
    bindTexture: vi.fn(), texParameteri: vi.fn(), bindFramebuffer: vi.fn(),
    framebufferTexture2D: vi.fn(), texImage2D: vi.fn(), viewport: vi.fn(),
    disable: vi.fn(), useProgram: vi.fn(), clearColor: vi.fn(), clear: vi.fn(),
    getUniformLocation: vi.fn((_program, name) => ({ name })),
    uniform1f: vi.fn(), uniform2f: vi.fn(), uniform4f: vi.fn(), uniform1i: vi.fn(),
    uniform4fv: vi.fn(), uniform1fv: vi.fn(), activeTexture: vi.fn(), drawArrays: vi.fn(),
    deleteProgram: vi.fn(), deleteTexture: vi.fn(), deleteFramebuffer: vi.fn(),
    deleteBuffer: vi.fn(), deleteVertexArray: vi.fn(), isContextLost: vi.fn(() => false),
    getExtension: vi.fn(() => ({ loseContext: vi.fn() })),
  };
}

const FRAME: YaniFieldFrame = {
  time: 10,
  dt: 1 / 60,
  phase: "idle",
  pointerX: 0.5,
  pointerY: 0.5,
  pointerVx: 0,
  pointerVy: 0,
  sceneShiftX: 0,
  sceneShiftY: 0,
  panels: [{ x: 0.2, y: 0.2, width: 0.4, height: 0.5, radius: 0.02 }],
};

describe("Yani Neko three-pass pipeline", () => {
  beforeEach(() => mocks.createProgram.mockImplementation(() => ({ id: Math.random() })));

  it("draws smoke, world and composite then releases every GPU resource once", () => {
    const gl = fakeGl();
    const pipeline = new YaniNekoPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      yaniQuality("high"),
    );
    pipeline.render(1000, 680, FRAME);
    expect(gl.drawArrays).toHaveBeenCalledTimes(3);
    expect(gl.texImage2D).toHaveBeenCalledTimes(3);
    expect(gl.uniform4fv).toHaveBeenCalledOnce();
    expect(gl.uniform1fv).toHaveBeenCalledOnce();
    expect(gl.bindFramebuffer).toHaveBeenCalledWith(gl.FRAMEBUFFER, null);
    pipeline.destroy();
    pipeline.destroy();
    expect(gl.deleteProgram).toHaveBeenCalledTimes(3);
    expect(gl.deleteTexture).toHaveBeenCalledTimes(3);
    expect(gl.deleteFramebuffer).toHaveBeenCalledTimes(3);
    expect(gl.deleteBuffer).toHaveBeenCalledOnce();
    expect(gl.deleteVertexArray).toHaveBeenCalledOnce();
  });

  it("abandons cleanup after context loss and exposes the harness hook", () => {
    const gl = fakeGl();
    const pipeline = new YaniNekoPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      yaniQuality("low"),
    );
    pipeline.loseContextForTesting();
    expect(gl.getExtension).toHaveBeenCalledWith("WEBGL_lose_context");
    pipeline.abandonAfterContextLoss();
    pipeline.destroy();
    expect(gl.deleteProgram).not.toHaveBeenCalled();
    expect(gl.deleteTexture).not.toHaveBeenCalled();
  });

  it("fails closed when a shader cannot compile", () => {
    const gl = fakeGl();
    mocks.createProgram.mockReturnValueOnce(null);
    expect(() => new YaniNekoPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      yaniQuality("high"),
    )).toThrow("smoke shader unavailable");
  });

  it("releases programs created before a later shader fails", () => {
    const gl = fakeGl();
    mocks.createProgram
      .mockReturnValueOnce({ id: 1 })
      .mockReturnValueOnce({ id: 2 })
      .mockReturnValueOnce(null);
    expect(() => new YaniNekoPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      yaniQuality("balanced"),
    )).toThrow("composite shader unavailable");
    expect(gl.deleteProgram).toHaveBeenCalledTimes(2);
    expect(gl.createTexture).not.toHaveBeenCalled();
  });
});
