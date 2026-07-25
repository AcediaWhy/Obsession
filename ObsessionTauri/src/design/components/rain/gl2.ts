// Тонкая WebGL2-обёртка для двухпроходного пайплайна «ночной поезд»:
// контекст, программы с логом ошибок, fullscreen-треугольник, FBO с мипами.
// Заменяет rain/gl.ts + rain/webgl.ts (WebGL1) — NPOT + мипы нужны для
// попиксельного фокуса (textureLod) в композите.

export function getContext2(
  canvas: HTMLCanvasElement,
  options: WebGLContextAttributes = {},
): WebGL2RenderingContext | null {
  try {
    return canvas.getContext("webgl2", options) as WebGL2RenderingContext | null;
  } catch {
    return null;
  }
}

function compileShader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.error("Rain GL2 shader:", gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

export function createProgram2(
  gl: WebGL2RenderingContext,
  vertSource: string,
  fragSource: string,
): WebGLProgram | null {
  const vert = compileShader(gl, gl.VERTEX_SHADER, vertSource);
  const frag = compileShader(gl, gl.FRAGMENT_SHADER, fragSource);
  if (!vert || !frag) {
    if (vert) gl.deleteShader(vert);
    if (frag) gl.deleteShader(frag);
    return null;
  }
  const program = gl.createProgram();
  if (!program) {
    gl.deleteShader(vert);
    gl.deleteShader(frag);
    return null;
  }
  gl.attachShader(program, vert);
  gl.attachShader(program, frag);
  gl.linkProgram(program);
  gl.detachShader(program, vert);
  gl.detachShader(program, frag);
  gl.deleteShader(vert);
  gl.deleteShader(frag);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.error("Rain GL2 link:", gl.getProgramInfoLog(program));
    gl.deleteProgram(program);
    return null;
  }
  return program;
}

/** Программа + кэш локаций uniform'ов. */
export class Gl2Program {
  readonly program: WebGLProgram;
  private readonly gl: WebGL2RenderingContext;
  private readonly locations = new Map<string, WebGLUniformLocation | null>();

  constructor(gl: WebGL2RenderingContext, program: WebGLProgram) {
    this.gl = gl;
    this.program = program;
  }

  use(): void {
    this.gl.useProgram(this.program);
  }

  private location(name: string): WebGLUniformLocation | null {
    const cached = this.locations.get(name);
    if (cached !== undefined) return cached;
    const location = this.gl.getUniformLocation(this.program, name);
    this.locations.set(name, location);
    return location;
  }

  set1f(name: string, value: number): void {
    this.gl.uniform1f(this.location(name), value);
  }

  set2f(name: string, x: number, y: number): void {
    this.gl.uniform2f(this.location(name), x, y);
  }

  set1i(name: string, value: number): void {
    this.gl.uniform1i(this.location(name), value);
  }

  dispose(): void {
    this.locations.clear();
    this.gl.deleteProgram(this.program);
  }
}

/** Fullscreen-треугольник в VAO; вершина — `layout(location = 0) vec2 a_position`. */
export function createFullscreenTriangle(gl: WebGL2RenderingContext): WebGLVertexArrayObject {
  const vao = gl.createVertexArray();
  if (!vao) throw new Error("Rain GL2: VAO недоступен");
  gl.bindVertexArray(vao);
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);
  return vao;
}

export type Gl2Fbo = {
  framebuffer: WebGLFramebuffer;
  texture: WebGLTexture;
  width: number;
  height: number;
};

/** RGBA8-таргет с мип-цепочкой (LINEAR_MIPMAP_LINEAR); NPOT легален в WebGL2. */
export function createFbo(gl: WebGL2RenderingContext, width: number, height: number): Gl2Fbo {
  const texture = gl.createTexture();
  const framebuffer = gl.createFramebuffer();
  if (!texture || !framebuffer) throw new Error("Rain GL2: FBO недоступен");
  const fbo: Gl2Fbo = { framebuffer, texture, width: 0, height: 0 };
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  resizeFbo(gl, fbo, width, height);
  gl.bindTexture(gl.TEXTURE_2D, null);
  return fbo;
}

export function resizeFbo(gl: WebGL2RenderingContext, fbo: Gl2Fbo, width: number, height: number): void {
  const w = Math.max(1, Math.round(width));
  const h = Math.max(1, Math.round(height));
  if (w === fbo.width && h === fbo.height) return;
  fbo.width = w;
  fbo.height = h;
  gl.bindTexture(gl.TEXTURE_2D, fbo.texture);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo.framebuffer);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, fbo.texture, 0);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
}

export function generateMips(gl: WebGL2RenderingContext, fbo: Gl2Fbo): void {
  gl.bindTexture(gl.TEXTURE_2D, fbo.texture);
  gl.generateMipmap(gl.TEXTURE_2D);
}

export function deleteFbo(gl: WebGL2RenderingContext, fbo: Gl2Fbo): void {
  gl.deleteTexture(fbo.texture);
  gl.deleteFramebuffer(fbo.framebuffer);
}
