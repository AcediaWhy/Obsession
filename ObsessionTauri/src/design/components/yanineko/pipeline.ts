import { createProgram2, getContext2 } from "../rain/gl2";
import { yaniMoodForPhase, yaniMoodValue } from "./state";
import {
  YANI_COMPOSITE_FRAGMENT_SHADER,
  YANI_FULLSCREEN_VERTEX_SHADER,
  YANI_SMOKE_FRAGMENT_SHADER,
  YANI_WORLD_FRAGMENT_SHADER,
} from "./shaders";
import type { YaniFieldFrame } from "./types";
import type { YaniQualityProfile } from "./quality";

type Program = { raw: WebGLProgram; uniforms: Map<string, WebGLUniformLocation | null> };
type Geometry = { vao: WebGLVertexArrayObject; buffer: WebGLBuffer };
type Target = {
  framebuffer: WebGLFramebuffer;
  texture: WebGLTexture;
  width: number;
  height: number;
};

function requireProgram(gl: WebGL2RenderingContext, fragment: string, label: string): Program {
  const raw = createProgram2(gl, YANI_FULLSCREEN_VERTEX_SHADER, fragment);
  if (!raw) throw new Error(`Yani Neko: ${label} shader unavailable`);
  return { raw, uniforms: new Map() };
}

function uniform(gl: WebGL2RenderingContext, program: Program, name: string) {
  if (program.uniforms.has(name)) return program.uniforms.get(name) ?? null;
  const value = gl.getUniformLocation(program.raw, name);
  program.uniforms.set(name, value);
  return value;
}

function createGeometry(gl: WebGL2RenderingContext): Geometry {
  const vao = gl.createVertexArray();
  const buffer = gl.createBuffer();
  if (!vao || !buffer) {
    if (buffer) gl.deleteBuffer(buffer);
    if (vao) gl.deleteVertexArray(vao);
    throw new Error("Yani Neko: fullscreen geometry unavailable");
  }
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);
  return { vao, buffer };
}

function createTarget(gl: WebGL2RenderingContext): Target {
  const framebuffer = gl.createFramebuffer();
  const texture = gl.createTexture();
  if (!framebuffer || !texture) {
    if (texture) gl.deleteTexture(texture);
    if (framebuffer) gl.deleteFramebuffer(framebuffer);
    throw new Error("Yani Neko: render target unavailable");
  }
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

function resizeTarget(gl: WebGL2RenderingContext, target: Target, width: number, height: number): boolean {
  const nextWidth = Math.max(1, Math.round(width));
  const nextHeight = Math.max(1, Math.round(height));
  if (target.width === nextWidth && target.height === nextHeight) return false;
  target.width = nextWidth;
  target.height = nextHeight;
  gl.bindTexture(gl.TEXTURE_2D, target.texture);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, nextWidth, nextHeight, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  return true;
}

function deleteTarget(gl: WebGL2RenderingContext, target: Target) {
  gl.deleteTexture(target.texture);
  gl.deleteFramebuffer(target.framebuffer);
}

export class YaniNekoPipeline {
  readonly gl: WebGL2RenderingContext;
  private readonly smokeProgram: Program;
  private readonly worldProgram: Program;
  private readonly compositeProgram: Program;
  private readonly geometry: Geometry;
  private readonly worldTarget: Target;
  private smokeRead: Target;
  private smokeWrite: Target;
  private quality: YaniQualityProfile;
  private resetSmoke = true;
  private disposed = false;

  constructor(canvas: HTMLCanvasElement, quality: YaniQualityProfile) {
    const gl = getContext2(canvas, {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: "high-performance",
    });
    if (!gl) throw new Error("Yani Neko requires WebGL2");
    this.gl = gl;
    this.quality = quality;
    let smokeProgram: Program | undefined;
    let worldProgram: Program | undefined;
    let compositeProgram: Program | undefined;
    let geometry: Geometry | undefined;
    let worldTarget: Target | undefined;
    let smokeRead: Target | undefined;
    let smokeWrite: Target | undefined;
    try {
      smokeProgram = requireProgram(gl, YANI_SMOKE_FRAGMENT_SHADER, "smoke");
      worldProgram = requireProgram(gl, YANI_WORLD_FRAGMENT_SHADER, "world");
      compositeProgram = requireProgram(gl, YANI_COMPOSITE_FRAGMENT_SHADER, "composite");
      geometry = createGeometry(gl);
      worldTarget = createTarget(gl);
      smokeRead = createTarget(gl);
      smokeWrite = createTarget(gl);
    } catch (error) {
      if (smokeWrite) deleteTarget(gl, smokeWrite);
      if (smokeRead) deleteTarget(gl, smokeRead);
      if (worldTarget) deleteTarget(gl, worldTarget);
      if (geometry) {
        gl.deleteBuffer(geometry.buffer);
        gl.deleteVertexArray(geometry.vao);
      }
      if (compositeProgram) gl.deleteProgram(compositeProgram.raw);
      if (worldProgram) gl.deleteProgram(worldProgram.raw);
      if (smokeProgram) gl.deleteProgram(smokeProgram.raw);
      throw error;
    }
    this.smokeProgram = smokeProgram;
    this.worldProgram = worldProgram;
    this.compositeProgram = compositeProgram;
    this.geometry = geometry;
    this.worldTarget = worldTarget;
    this.smokeRead = smokeRead;
    this.smokeWrite = smokeWrite;
  }

  setQuality(quality: YaniQualityProfile): void {
    this.quality = quality;
  }

  private clearTarget(target: Target): void {
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, target.framebuffer);
    gl.viewport(0, 0, target.width, target.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
  }

  render(width: number, height: number, frame: YaniFieldFrame): void {
    if (this.disposed) return;
    const gl = this.gl;
    const worldWidth = Math.max(1, Math.round(width * this.quality.worldScale));
    const worldHeight = Math.max(1, Math.round(height * this.quality.worldScale));
    const smokeWidth = Math.max(1, Math.round(width * this.quality.smokeScale));
    const smokeHeight = Math.max(1, Math.round(height * this.quality.smokeScale));
    resizeTarget(gl, this.worldTarget, worldWidth, worldHeight);
    const readResized = resizeTarget(gl, this.smokeRead, smokeWidth, smokeHeight);
    const writeResized = resizeTarget(gl, this.smokeWrite, smokeWidth, smokeHeight);
    const smokeResized = readResized || writeResized;
    if (smokeResized) {
      this.clearTarget(this.smokeRead);
      this.clearTarget(this.smokeWrite);
      this.resetSmoke = true;
    }
    const mood = yaniMoodValue(yaniMoodForPhase(frame.phase));

    // Pass 1: feedback smoke advection.
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.smokeWrite.framebuffer);
    gl.viewport(0, 0, smokeWidth, smokeHeight);
    gl.disable(gl.BLEND);
    gl.useProgram(this.smokeProgram.raw);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.smokeRead.texture);
    gl.uniform1i(uniform(gl, this.smokeProgram, "u_previous"), 0);
    gl.uniform2f(uniform(gl, this.smokeProgram, "u_resolution"), smokeWidth, smokeHeight);
    gl.uniform4f(
      uniform(gl, this.smokeProgram, "u_pointer"),
      frame.pointerX,
      frame.pointerY,
      frame.pointerVx,
      frame.pointerVy,
    );
    gl.uniform1f(uniform(gl, this.smokeProgram, "u_time"), frame.time);
    gl.uniform1f(uniform(gl, this.smokeProgram, "u_dt"), frame.dt);
    gl.uniform1f(uniform(gl, this.smokeProgram, "u_mood"), mood);
    gl.uniform1f(uniform(gl, this.smokeProgram, "u_reset"), this.resetSmoke ? 1 : 0);
    gl.uniform1i(uniform(gl, this.smokeProgram, "u_octaves"), this.quality.noiseOctaves);
    gl.bindVertexArray(this.geometry.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    this.resetSmoke = false;
    [this.smokeRead, this.smokeWrite] = [this.smokeWrite, this.smokeRead];

    // Pass 2: procedural room world.
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.worldTarget.framebuffer);
    gl.viewport(0, 0, worldWidth, worldHeight);
    gl.useProgram(this.worldProgram.raw);
    gl.uniform2f(uniform(gl, this.worldProgram, "u_resolution"), worldWidth, worldHeight);
    gl.uniform2f(uniform(gl, this.worldProgram, "u_scene_shift"), frame.sceneShiftX, frame.sceneShiftY);
    gl.uniform1f(uniform(gl, this.worldProgram, "u_time"), frame.time);
    gl.uniform1f(uniform(gl, this.worldProgram, "u_mood"), mood);
    gl.uniform1i(uniform(gl, this.worldProgram, "u_dust_count"), this.quality.dustCount);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    // Pass 3: smoke, bloom, grading and panel-aware optical composite.
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, width, height);
    gl.useProgram(this.compositeProgram.raw);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.worldTarget.texture);
    gl.uniform1i(uniform(gl, this.compositeProgram, "u_world"), 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.smokeRead.texture);
    gl.uniform1i(uniform(gl, this.compositeProgram, "u_smoke"), 1);
    gl.uniform2f(uniform(gl, this.compositeProgram, "u_resolution"), width, height);
    gl.uniform2f(uniform(gl, this.compositeProgram, "u_smoke_resolution"), smokeWidth, smokeHeight);
    gl.uniform1f(uniform(gl, this.compositeProgram, "u_time"), frame.time);
    gl.uniform1f(uniform(gl, this.compositeProgram, "u_mood"), mood);
    const panels = frame.panels.slice(0, this.quality.panelCount);
    const panelValues = new Float32Array(12 * 4);
    const radiusValues = new Float32Array(12);
    panels.forEach((panel, index) => {
      panelValues.set([panel.x, panel.y, panel.width, panel.height], index * 4);
      radiusValues[index] = panel.radius;
    });
    gl.uniform1i(uniform(gl, this.compositeProgram, "u_panel_count"), panels.length);
    gl.uniform4fv(uniform(gl, this.compositeProgram, "u_panels[0]"), panelValues);
    gl.uniform1fv(uniform(gl, this.compositeProgram, "u_panel_radii[0]"), radiusValues);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
  }

  abandonAfterContextLoss(): void {
    this.disposed = true;
  }

  loseContextForTesting(): void {
    this.gl.getExtension("WEBGL_lose_context")?.loseContext();
  }

  destroy(): void {
    if (this.disposed || this.gl.isContextLost()) return;
    this.disposed = true;
    const gl = this.gl;
    gl.deleteProgram(this.smokeProgram.raw);
    gl.deleteProgram(this.worldProgram.raw);
    gl.deleteProgram(this.compositeProgram.raw);
    deleteTarget(gl, this.worldTarget);
    deleteTarget(gl, this.smokeRead);
    deleteTarget(gl, this.smokeWrite);
    gl.deleteBuffer(this.geometry.buffer);
    gl.deleteVertexArray(this.geometry.vao);
    // Контекст отдаём сразу, как в rain/pipeline.ts: без этого он ждёт сборки
    // мусора, которой в трее не бывает, и занимает слот из лимита Chromium.
    gl.getExtension("WEBGL_lose_context")?.loseContext();
  }
}
