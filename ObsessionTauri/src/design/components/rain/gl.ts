// Порт gl-obj.js — объект-обёртка вокруг программы и рисования квада.
import * as WebGL from "./webgl";

export class GL {
  canvas: HTMLCanvasElement;
  gl: WebGLRenderingContext;
  program: WebGLProgram;
  width: number;
  height: number;

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
    this.program = WebGL.createProgram(gl, vert, frag)!;
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
    WebGL.setRectangle(this.gl, -1, -1, 2, 2);
    this.gl.drawArrays(this.gl.TRIANGLES, 0, 6);
  }
}
