import { createProgram2, Gl2Program, getContext2 } from "../rain/gl2";
import { createObsessionPlate } from "./plate";
import type { ObsessionQualityProfile } from "./quality";
import { OBSESSION_FRAGMENT_SHADER, OBSESSION_VERTEX_SHADER } from "./shaders";

export type ObsessionFrame = {
  time: number;
  phase: number;
  phaseAge: number;
  focusX: number;
  focusY: number;
  pointerX: number;
  pointerY: number;
  capture: number;
  gazeX: number;
  gazeY: number;
  lidOpen: number;
  pupilScale: number;
  bodyTension: number;
  irisRotation: number;
  highlightPhase: number;
  fixation: number;
  faultSplit: number;
};

type Geometry = { vao: WebGLVertexArrayObject; buffer: WebGLBuffer };

function createGeometry(gl: WebGL2RenderingContext): Geometry {
  const vao = gl.createVertexArray();
  const buffer = gl.createBuffer();
  if (!vao || !buffer) throw new Error("Obsession: fullscreen geometry unavailable");
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);
  gl.bindBuffer(gl.ARRAY_BUFFER, null);
  return { vao, buffer };
}

function uploadTexture(
  gl: WebGL2RenderingContext,
  size: number,
  pixels: Uint8Array,
): WebGLTexture {
  const texture = gl.createTexture();
  if (!texture) throw new Error("Obsession: optical texture unavailable");
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, size, size, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
  gl.generateMipmap(gl.TEXTURE_2D);
  return texture;
}

export class ObsessionPipeline {
  readonly gl: WebGL2RenderingContext;
  private readonly program: Gl2Program;
  private readonly geometry: Geometry;
  private readonly plateTexture: WebGLTexture;
  private readonly normalTexture: WebGLTexture;
  private quality: ObsessionQualityProfile;
  private disposed = false;

  constructor(canvas: HTMLCanvasElement, quality: ObsessionQualityProfile) {
    const gl = getContext2(canvas, {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: "high-performance",
    });
    if (!gl) throw new Error("Obsession requires WebGL2");
    const rawProgram = createProgram2(gl, OBSESSION_VERTEX_SHADER, OBSESSION_FRAGMENT_SHADER);
    if (!rawProgram) throw new Error("Obsession shader compilation failed");
    this.gl = gl;
    this.program = new Gl2Program(gl, rawProgram);
    this.geometry = createGeometry(gl);
    const plate = createObsessionPlate();
    this.plateTexture = uploadTexture(gl, plate.size, plate.color);
    this.normalTexture = uploadTexture(gl, plate.size, plate.normal);
    this.quality = quality;
  }

  setQuality(quality: ObsessionQualityProfile): void {
    this.quality = quality;
  }

  render(width: number, height: number, frame: ObsessionFrame): void {
    if (this.disposed) return;
    const gl = this.gl;
    gl.viewport(0, 0, width, height);
    gl.disable(gl.BLEND);
    this.program.use();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.plateTexture);
    this.program.set1i("u_plate", 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.normalTexture);
    this.program.set1i("u_normal", 1);
    this.program.set2f("u_resolution", width, height);
    this.program.set2f("u_focus", frame.focusX, frame.focusY);
    this.program.set2f("u_pointer", frame.pointerX, frame.pointerY);
    this.program.set1f("u_time", frame.time);
    this.program.set1f("u_phase", frame.phase);
    this.program.set1f("u_phase_age", frame.phaseAge);
    this.program.set1f("u_threads", this.quality.threadCount);
    this.program.set1f("u_caustics", this.quality.caustics ? 1 : 0);
    this.program.set1f("u_aberration", this.quality.aberration);
    this.program.set1f("u_capture", frame.capture);
    this.program.set2f("u_gaze", frame.gazeX, frame.gazeY);
    this.program.set1f("u_lid_open", frame.lidOpen);
    this.program.set1f("u_pupil_scale", frame.pupilScale);
    this.program.set1f("u_body_tension", frame.bodyTension);
    this.program.set1f("u_iris_rotation", frame.irisRotation);
    this.program.set1f("u_highlight_phase", frame.highlightPhase);
    this.program.set1f("u_fixation", frame.fixation);
    this.program.set1f("u_fault_split", frame.faultSplit);
    gl.bindVertexArray(this.geometry.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
  }

  abandonAfterContextLoss(): void {
    this.disposed = true;
  }

  destroy(): void {
    if (this.disposed || this.gl.isContextLost()) return;
    this.disposed = true;
    this.program.dispose();
    this.gl.deleteTexture(this.plateTexture);
    this.gl.deleteTexture(this.normalTexture);
    this.gl.deleteBuffer(this.geometry.buffer);
    this.gl.deleteVertexArray(this.geometry.vao);
  }
}
