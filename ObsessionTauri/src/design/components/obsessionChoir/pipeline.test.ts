import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ createProgram: vi.fn() }));

vi.mock("../rain/gl2", () => ({
  getContext2: (canvas: HTMLCanvasElement) =>
    (canvas as unknown as { __gl: WebGL2RenderingContext }).__gl,
  createProgram2: () => mocks.createProgram(),
}));

import { ObsessionChoirPipeline } from "./pipeline";
import { obsessionChoirQuality } from "./quality";

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
    SRC_ALPHA: 17,
    ONE: 18,
    TRIANGLES: 19,
    TEXTURE0: 20,
    createVertexArray: vi.fn(() => ({ id: ++id })),
    createBuffer: vi.fn(() => ({ id: ++id })),
    createFramebuffer: vi.fn(() => ({ id: ++id })),
    createTexture: vi.fn(() => ({ id: ++id })),
    bindVertexArray: vi.fn(), bindBuffer: vi.fn(), bufferData: vi.fn(),
    enableVertexAttribArray: vi.fn(), vertexAttribPointer: vi.fn(),
    bindTexture: vi.fn(), texParameteri: vi.fn(), bindFramebuffer: vi.fn(),
    framebufferTexture2D: vi.fn(), texImage2D: vi.fn(), viewport: vi.fn(),
    disable: vi.fn(), enable: vi.fn(), blendFunc: vi.fn(), useProgram: vi.fn(),
    getUniformLocation: vi.fn((_program, name) => ({ name })),
    uniform1f: vi.fn(), uniform2f: vi.fn(), uniform4f: vi.fn(), uniform1i: vi.fn(),
    uniform4fv: vi.fn(), uniform1fv: vi.fn(), activeTexture: vi.fn(), drawArrays: vi.fn(),
    deleteProgram: vi.fn(), deleteTexture: vi.fn(), deleteFramebuffer: vi.fn(),
    deleteBuffer: vi.fn(), deleteVertexArray: vi.fn(), isContextLost: vi.fn(() => false),
    // destroy() отдаёт контекст сразу, а не ждёт сборки мусора — расширение
    // WEBGL_lose_context должно быть в заглушке, иначе уборка не проверяется.
    getExtension: vi.fn(() => ({ loseContext: vi.fn() })),
  };
}

const FRAME = {
  time: 10,
  phase: "idle" as const,
  focusX: 0.82,
  focusY: 0.66,
  pointerX: 0,
  pointerY: 0,
  masterGazeX: 0.1,
  masterGazeY: -0.1,
  masterLidOpen: 0.9,
  masterBodyX: 0.55,
  masterBodyY: 0.46,
  masterRoll: 0.52,
  masterPulse: 0.7,
  pupilScale: 0.6,
  chorusReveal: 0.1,
  chorusAlignment: 0,
  lineTension: 0.5,
  lineFlow: 0.65,
  apertureOpen: 0.8,
  carmineDepth: 0.5,
  faultShear: 0,
  ritual: 0,
  panels: [{ x: 0.2, y: 0.2, width: 0.4, height: 0.5, radius: 0.02 }],
};

describe("Black Choir two-pass pipeline", () => {
  beforeEach(() => mocks.createProgram.mockImplementation(() => ({ id: Math.random() })));

  it("draws world, ribbon geometry and panel composite then releases every resource", () => {
    const gl = fakeGl();
    const pipeline = new ObsessionChoirPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      obsessionChoirQuality("high"),
    );
    pipeline.render(1000, 680, FRAME);
    expect(gl.drawArrays).toHaveBeenCalledTimes(3);
    expect(gl.uniform4fv).toHaveBeenCalledOnce();
    expect(gl.uniform1fv).toHaveBeenCalledOnce();
    expect(gl.uniform4f).toHaveBeenCalledTimes(2);
    expect(gl.bindFramebuffer).toHaveBeenCalledWith(gl.FRAMEBUFFER, null);
    pipeline.destroy();
    pipeline.destroy();
    expect(gl.deleteProgram).toHaveBeenCalledTimes(3);
    expect(gl.deleteTexture).toHaveBeenCalledOnce();
    expect(gl.deleteFramebuffer).toHaveBeenCalledOnce();
    expect(gl.deleteBuffer).toHaveBeenCalledTimes(2);
    // Контекст отпущен ровно один раз, повторный destroy() его не трогает.
    expect(gl.getExtension).toHaveBeenCalledWith("WEBGL_lose_context");
    expect(gl.getExtension).toHaveBeenCalledOnce();
    expect(gl.deleteVertexArray).toHaveBeenCalledTimes(2);
  });

  it("avoids invalid cleanup after context loss", () => {
    const gl = fakeGl();
    const pipeline = new ObsessionChoirPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      obsessionChoirQuality("low"),
    );
    pipeline.abandonAfterContextLoss();
    pipeline.destroy();
    expect(gl.deleteProgram).not.toHaveBeenCalled();
    expect(gl.deleteTexture).not.toHaveBeenCalled();
  });

  it("fails closed when either shader pass cannot compile", () => {
    const gl = fakeGl();
    mocks.createProgram.mockReturnValueOnce(null);
    expect(() => new ObsessionChoirPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      obsessionChoirQuality("high"),
    )).toThrow("world shader unavailable");
  });

  it("uses the low-tier segment and curve budget in the actual draw", () => {
    const gl = fakeGl();
    const quality = obsessionChoirQuality("low");
    const pipeline = new ObsessionChoirPipeline(
      { __gl: gl } as unknown as HTMLCanvasElement,
      quality,
    );
    pipeline.render(800, 600, FRAME);
    expect(gl.drawArrays).toHaveBeenNthCalledWith(
      2,
      gl.TRIANGLES,
      0,
      quality.curveCount * quality.curveSegments * 6,
    );
  });
});
