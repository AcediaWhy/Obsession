import { createProgram2, getContext2 } from "../rain/gl2";
import { CHOIR_CURVES, createChoirRibbonGeometry } from "./geometry";
import type { ObsessionChoirQuality } from "./quality";
import {
  CHOIR_COMPOSITE_FRAGMENT_SHADER,
  CHOIR_FULLSCREEN_VERTEX_SHADER,
  CHOIR_RIBBON_FRAGMENT_SHADER,
  CHOIR_RIBBON_VERTEX_SHADER,
  CHOIR_WORLD_FRAGMENT_SHADER,
} from "./shaders";
import type { ObsessionChoirFrame } from "./types";

type Program = {
  raw: WebGLProgram;
  uniforms: Map<string, WebGLUniformLocation | null>;
};

type FullscreenGeometry = { vao: WebGLVertexArrayObject; buffer: WebGLBuffer };
type RibbonGeometry = {
  vao: WebGLVertexArrayObject;
  buffer: WebGLBuffer;
  curveVertexOffsets: readonly number[];
};
type RenderTarget = {
  framebuffer: WebGLFramebuffer;
  texture: WebGLTexture;
  width: number;
  height: number;
};

function requireProgram(
  gl: WebGL2RenderingContext,
  vertex: string,
  fragment: string,
  label: string,
): Program {
  const raw = createProgram2(gl, vertex, fragment);
  if (!raw) throw new Error(`Black Choir: ${label} shader unavailable`);
  return { raw, uniforms: new Map() };
}

function location(gl: WebGL2RenderingContext, program: Program, name: string) {
  if (program.uniforms.has(name)) return program.uniforms.get(name) ?? null;
  const next = gl.getUniformLocation(program.raw, name);
  program.uniforms.set(name, next);
  return next;
}

function createFullscreenGeometry(gl: WebGL2RenderingContext): FullscreenGeometry {
  const vao = gl.createVertexArray();
  const buffer = gl.createBuffer();
  if (!vao || !buffer) throw new Error("Black Choir: fullscreen geometry unavailable");
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);
  return { vao, buffer };
}

function createRibbonGeometry(gl: WebGL2RenderingContext, segments: number): RibbonGeometry {
  const geometry = createChoirRibbonGeometry(segments);
  const vao = gl.createVertexArray();
  const buffer = gl.createBuffer();
  if (!vao || !buffer) throw new Error("Black Choir: ribbon geometry unavailable");
  const stride = geometry.vertexStride * Float32Array.BYTES_PER_ELEMENT;
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, geometry.vertices, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, stride, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 2, gl.FLOAT, false, stride, 2 * Float32Array.BYTES_PER_ELEMENT);
  gl.enableVertexAttribArray(2);
  gl.vertexAttribPointer(2, 4, gl.FLOAT, false, stride, 4 * Float32Array.BYTES_PER_ELEMENT);
  gl.bindVertexArray(null);
  return { vao, buffer, curveVertexOffsets: geometry.curveVertexOffsets };
}

function createRenderTarget(gl: WebGL2RenderingContext): RenderTarget {
  const framebuffer = gl.createFramebuffer();
  const texture = gl.createTexture();
  if (!framebuffer || !texture) throw new Error("Black Choir: render target unavailable");
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  return { framebuffer, texture, width: 0, height: 0 };
}

function resizeRenderTarget(gl: WebGL2RenderingContext, target: RenderTarget, width: number, height: number) {
  if (target.width === width && target.height === height) return;
  target.width = width;
  target.height = height;
  gl.bindTexture(gl.TEXTURE_2D, target.texture);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
}

function phaseValue(phase: ObsessionChoirFrame["phase"]): number {
  return phase === "engaging" ? 1 : phase === "scanning" ? 2 : phase === "focused" ? 3 : phase === "fault" ? 4 : 0;
}

export class ObsessionChoirPipeline {
  readonly gl: WebGL2RenderingContext;
  private readonly worldProgram: Program;
  private readonly ribbonProgram: Program;
  private readonly compositeProgram: Program;
  private readonly fullscreen: FullscreenGeometry;
  private readonly ribbons: RibbonGeometry;
  private readonly target: RenderTarget;
  private quality: ObsessionChoirQuality;
  private disposed = false;

  constructor(canvas: HTMLCanvasElement, quality: ObsessionChoirQuality) {
    const gl = getContext2(canvas, {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
    });
    if (!gl) throw new Error("Black Choir requires WebGL2");
    this.gl = gl;
    this.quality = quality;
    this.worldProgram = requireProgram(gl, CHOIR_FULLSCREEN_VERTEX_SHADER, CHOIR_WORLD_FRAGMENT_SHADER, "world");
    this.ribbonProgram = requireProgram(gl, CHOIR_RIBBON_VERTEX_SHADER, CHOIR_RIBBON_FRAGMENT_SHADER, "ribbon");
    this.compositeProgram = requireProgram(gl, CHOIR_FULLSCREEN_VERTEX_SHADER, CHOIR_COMPOSITE_FRAGMENT_SHADER, "composite");
    this.fullscreen = createFullscreenGeometry(gl);
    this.ribbons = createRibbonGeometry(gl, quality.curveSegments);
    this.target = createRenderTarget(gl);
  }

  setQuality(quality: ObsessionChoirQuality) {
    this.quality = quality;
  }

  render(width: number, height: number, frame: ObsessionChoirFrame) {
    if (this.disposed) return;
    const gl = this.gl;
    const targetWidth = Math.max(1, Math.round(width * this.quality.resolutionScale));
    const targetHeight = Math.max(1, Math.round(height * this.quality.resolutionScale));
    resizeRenderTarget(gl, this.target, targetWidth, targetHeight);

    gl.bindFramebuffer(gl.FRAMEBUFFER, this.target.framebuffer);
    gl.viewport(0, 0, targetWidth, targetHeight);
    gl.disable(gl.BLEND);
    gl.useProgram(this.worldProgram.raw);
    gl.uniform2f(location(gl, this.worldProgram, "u_resolution"), targetWidth, targetHeight);
    gl.uniform2f(location(gl, this.worldProgram, "u_focus"), frame.focusX, frame.focusY);
    gl.uniform2f(location(gl, this.worldProgram, "u_gaze"), frame.masterGazeX, frame.masterGazeY);
    gl.uniform4f(
      location(gl, this.worldProgram, "u_body"),
      frame.masterBodyX,
      frame.masterBodyY,
      frame.masterRoll,
      frame.masterPulse,
    );
    gl.uniform1f(location(gl, this.worldProgram, "u_time"), frame.time);
    gl.uniform1f(location(gl, this.worldProgram, "u_phase"), phaseValue(frame.phase));
    gl.uniform1f(location(gl, this.worldProgram, "u_lid_open"), frame.masterLidOpen);
    gl.uniform1f(location(gl, this.worldProgram, "u_pupil_scale"), frame.pupilScale);
    gl.uniform1f(location(gl, this.worldProgram, "u_line_flow"), frame.lineFlow);
    gl.uniform1f(location(gl, this.worldProgram, "u_chorus_reveal"), frame.chorusReveal);
    gl.uniform1f(location(gl, this.worldProgram, "u_chorus_alignment"), frame.chorusAlignment);
    gl.uniform1f(location(gl, this.worldProgram, "u_carmine_depth"), frame.carmineDepth);
    gl.uniform1f(location(gl, this.worldProgram, "u_fault_shear"), frame.faultShear);
    gl.uniform1f(location(gl, this.worldProgram, "u_ritual"), frame.ritual);
    gl.bindVertexArray(this.fullscreen.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
    gl.useProgram(this.ribbonProgram.raw);
    gl.uniform2f(location(gl, this.ribbonProgram, "u_resolution"), targetWidth, targetHeight);
    gl.uniform1f(location(gl, this.ribbonProgram, "u_time"), frame.time);
    gl.uniform1f(location(gl, this.ribbonProgram, "u_tension"), frame.lineTension);
    gl.uniform1f(location(gl, this.ribbonProgram, "u_line_flow"), frame.lineFlow);
    gl.uniform1f(location(gl, this.ribbonProgram, "u_fault_shear"), frame.faultShear);
    gl.uniform1f(location(gl, this.ribbonProgram, "u_carmine_depth"), frame.carmineDepth);
    gl.uniform1f(location(gl, this.ribbonProgram, "u_ritual"), frame.ritual);
    gl.uniform2f(location(gl, this.ribbonProgram, "u_focus"), frame.focusX, frame.focusY);
    gl.uniform4f(
      location(gl, this.ribbonProgram, "u_body"),
      frame.masterBodyX,
      frame.masterBodyY,
      frame.masterRoll,
      frame.masterPulse,
    );
    gl.uniform1f(location(gl, this.ribbonProgram, "u_lid_open"), frame.masterLidOpen);
    gl.bindVertexArray(this.ribbons.vao);
    const visibleCurves = Math.min(this.quality.curveCount, CHOIR_CURVES.length);
    const endVertex = this.ribbons.curveVertexOffsets[visibleCurves];
    gl.drawArrays(gl.TRIANGLES, 0, endVertex);

    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, width, height);
    gl.disable(gl.BLEND);
    gl.useProgram(this.compositeProgram.raw);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.target.texture);
    gl.uniform1i(location(gl, this.compositeProgram, "u_scene"), 0);
    gl.uniform2f(location(gl, this.compositeProgram, "u_resolution"), width, height);
    gl.uniform2f(location(gl, this.compositeProgram, "u_pointer"), frame.pointerX, frame.pointerY);
    gl.uniform1f(location(gl, this.compositeProgram, "u_time"), frame.time);
    gl.uniform1f(location(gl, this.compositeProgram, "u_refraction"), this.quality.refraction);
    gl.uniform1f(location(gl, this.compositeProgram, "u_aberration"), this.quality.aberration);
    gl.uniform1f(location(gl, this.compositeProgram, "u_caustics"), this.quality.caustics ? 1 : 0);
    const panels = frame.panels.slice(0, this.quality.panelCount);
    const panelValues = new Float32Array(12 * 4);
    const radiusValues = new Float32Array(12);
    panels.forEach((panel, index) => {
      panelValues.set([panel.x, panel.y, panel.width, panel.height], index * 4);
      radiusValues[index] = panel.radius;
    });
    gl.uniform1i(location(gl, this.compositeProgram, "u_panel_count"), panels.length);
    gl.uniform4fv(location(gl, this.compositeProgram, "u_panels[0]"), panelValues);
    gl.uniform1fv(location(gl, this.compositeProgram, "u_panel_radii[0]"), radiusValues);
    gl.bindVertexArray(this.fullscreen.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
  }

  abandonAfterContextLoss() {
    this.disposed = true;
  }

  /** Жив ли пайплайн для персистентной сессии (см. gl/persistentGlSession). */
  isAlive(): boolean {
    return !this.disposed && !this.gl.isContextLost();
  }

  destroy() {
    if (this.disposed || this.gl.isContextLost()) return;
    this.disposed = true;
    const gl = this.gl;
    gl.deleteProgram(this.worldProgram.raw);
    gl.deleteProgram(this.ribbonProgram.raw);
    gl.deleteProgram(this.compositeProgram.raw);
    gl.deleteTexture(this.target.texture);
    gl.deleteFramebuffer(this.target.framebuffer);
    gl.deleteBuffer(this.fullscreen.buffer);
    gl.deleteVertexArray(this.fullscreen.vao);
    gl.deleteBuffer(this.ribbons.buffer);
    gl.deleteVertexArray(this.ribbons.vao);
    // Контекст отдаём сразу, как в rain/pipeline.ts: deleteProgram/deleteTexture
    // освобождают объекты, но сам контекст с его декодером команд в GPU-процессе
    // живёт до сборки мусора — а в трее её не бывает.
    gl.getExtension("WEBGL_lose_context")?.loseContext();
  }
}
