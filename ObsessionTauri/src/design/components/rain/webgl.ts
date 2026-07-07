// Порт webgl.js из codrops/RainEffect — тонкая обёртка над WebGL1.
// Дословно, с типами; логика не менялась.

export function getContext(
  canvas: HTMLCanvasElement,
  options: WebGLContextAttributes = {},
): WebGLRenderingContext | null {
  const names = ["webgl", "experimental-webgl"];
  let context: WebGLRenderingContext | null = null;
  names.some((name) => {
    try {
      context = canvas.getContext(name, options) as WebGLRenderingContext | null;
    } catch {
      /* пробуем следующее имя */
    }
    return context != null;
  });
  return context;
}

export function createProgram(
  gl: WebGLRenderingContext,
  vertexScript: string,
  fragScript: string,
): WebGLProgram | null {
  const vertexShader = createShader(gl, vertexScript, gl.VERTEX_SHADER);
  const fragShader = createShader(gl, fragScript, gl.FRAGMENT_SHADER);
  if (!vertexShader || !fragShader) return null;

  const program = gl.createProgram()!;
  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragShader);
  gl.linkProgram(program);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.error("Error in program linking:", gl.getProgramInfoLog(program));
    gl.deleteProgram(program);
    return null;
  }

  const positionLocation = gl.getAttribLocation(program, "a_position");
  const texCoordLocation = gl.getAttribLocation(program, "a_texCoord");

  const texCoordBuffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, texCoordBuffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW,
  );
  gl.enableVertexAttribArray(texCoordLocation);
  gl.vertexAttribPointer(texCoordLocation, 2, gl.FLOAT, false, 0, 0);

  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.enableVertexAttribArray(positionLocation);
  gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);

  return program;
}

export function createShader(
  gl: WebGLRenderingContext,
  script: string,
  type: number,
): WebGLShader | null {
  const shader = gl.createShader(type)!;
  gl.shaderSource(shader, script);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.error("Error compiling shader:", gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

type TexSource = TexImageSource | null;

export function createTexture(gl: WebGLRenderingContext, source: TexSource, i: number): WebGLTexture {
  const texture = gl.createTexture()!;
  activeTexture(gl, i);
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  if (source != null) updateTexture(gl, source);
  return texture;
}

export function createUniform(
  gl: WebGLRenderingContext,
  program: WebGLProgram,
  type: string,
  name: string,
  ...args: number[]
): void {
  const location = gl.getUniformLocation(program, "u_" + name);
  // gl.uniform1f/2f/1i и т.п.
  (gl as unknown as Record<string, (...a: unknown[]) => void>)["uniform" + type](location, ...args);
}

export function activeTexture(gl: WebGLRenderingContext, i: number): void {
  (gl as unknown as Record<string, number>);
  gl.activeTexture((gl as unknown as Record<string, number>)["TEXTURE" + i]);
}

export function updateTexture(gl: WebGLRenderingContext, source: TexImageSource): void {
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
}

export function setRectangle(
  gl: WebGLRenderingContext,
  x: number,
  y: number,
  width: number,
  height: number,
): void {
  const x1 = x;
  const x2 = x + width;
  const y1 = y;
  const y2 = y + height;
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([x1, y1, x2, y1, x1, y2, x1, y2, x2, y1, x2, y2]),
    gl.STATIC_DRAW,
  );
}
