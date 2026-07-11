// Порт gl-obj.js — объект-обёртка вокруг программы и рисования квада.
import * as WebGL from "./webgl";

export class GL {
  canvas: HTMLCanvasElement;
  gl: WebGLRenderingContext;
  program: WebGLProgram;
  width: number;
  height: number;
  private texCoordBuffer: WebGLBuffer | null;
  private positionBuffer: WebGLBuffer | null;
  private quadReady = false;

  constructor(
    canvas: HTMLCanvasElement,
    options: WebGLContextAttributes,
    vert: string,
    frag: string,
  ) {
    this.canvas = canvas;
    this.width = canvas.width;
    this.height = canvas.height;
    const gl = WebGL.getContext(canvas, options);
    if (!gl) throw new Error("WebGL недоступен");
    this.gl = gl;
    const built = WebGL.createProgram(gl, vert, frag);
    if (!built) throw new Error("WebGL: не удалось собрать программу");
    this.program = built.program;
    this.texCoordBuffer = built.texCoordBuffer;
    this.positionBuffer = built.positionBuffer;
    this.useProgram(this.program);
  }

  useProgram(program: WebGLProgram) {
    this.program = program;
    this.gl.useProgram(program);
  }

  createTexture(source: TexImageSource | null, i: number) {
    return WebGL.createTexture(this.gl, source, i);
  }

  createUniform(type: string, name: string, ...v: number[]) {
    WebGL.createUniform(this.gl, this.program, type, name, ...v);
  }

  activeTexture(i: number) {
    WebGL.activeTexture(this.gl, i);
  }

  updateTexture(source: TexImageSource) {
    WebGL.updateTexture(this.gl, source);
  }

  draw() {
    // Полноэкранный квад статичен — заливаем позиционный буфер один раз,
    // а не перезаливаем Float32Array каждый кадр.
    if (!this.quadReady) {
      WebGL.setRectangle(this.gl, -1, -1, 2, 2);
      this.quadReady = true;
    }
    this.gl.drawArrays(this.gl.TRIANGLES, 0, 6);
  }

  /** Освобождает GPU-ресурсы: буферы, программу и контекст.
   *  Без этого каждый вход/выход темы Rain течёт WebGL-контекст (браузер
   *  держит ~16 живых) + текстуры. Звать при размонтировании. */
  destroy() {
    const gl = this.gl;
    if (this.texCoordBuffer) gl.deleteBuffer(this.texCoordBuffer);
    if (this.positionBuffer) gl.deleteBuffer(this.positionBuffer);
    if (this.program) gl.deleteProgram(this.program);
    const lose = gl.getExtension("WEBGL_lose_context");
    if (lose) lose.loseContext();
    this.quadReady = false;
  }
}
