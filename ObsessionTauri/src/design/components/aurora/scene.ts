import type { QualityTier } from "../../render";
import { auroraFragment, auroraVertex } from "./shaders";

export type AuroraPointer = { x: number; y: number; strength: number };
export type AuroraScene = {
  resize(width: number, height: number, quality: QualityTier): void;
  draw(time: number, activity: number, activationAge: number, pointer?: AuroraPointer): void;
  dispose(): void;
};
const qualityScale = { high: 0.9, balanced: 0.64, low: 0.45 } as const;

export function createAuroraScene(canvas: HTMLCanvasElement): AuroraScene | null {
  let gl: WebGLRenderingContext | null;
  try {
    gl = canvas.getContext("webgl", {
      alpha: false, antialias: false, depth: false, stencil: false,
      premultipliedAlpha: false, preserveDrawingBuffer: false,
    });
  } catch { return null; }
  if (!gl) return null;
  let program: WebGLProgram | null = null;
  let buffer: WebGLBuffer | null = null;
  let noiseTexture: WebGLTexture | null = null;
  const shaders: WebGLShader[] = [];
  let disposed = false;
  let resourcesLost = false;
  const onLost = () => { resourcesLost = true; };
  canvas.addEventListener("webglcontextlost", onLost);
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    canvas.removeEventListener("webglcontextlost", onLost);
    if (!resourcesLost) {
      if (noiseTexture) gl.deleteTexture(noiseTexture);
      if (buffer) gl.deleteBuffer(buffer);
      if (program) gl.deleteProgram(program);
      shaders.forEach((shader) => gl.deleteShader(shader));
    }
    // StrictMode probes a connected canvas; release its context after removal.
    if (!canvas.isConnected) gl.getExtension("WEBGL_lose_context")?.loseContext();
    canvas.width = 0; canvas.height = 0;
  };
  try {
    if (!gl.getShaderPrecisionFormat(gl.FRAGMENT_SHADER, gl.HIGH_FLOAT)?.precision) { dispose(); return null; }
    const compile = (type: number, source: string) => {
      const shader = gl.createShader(type);
      if (!shader) throw new Error("Aurora shader allocation failed");
      shaders.push(shader);
      gl.shaderSource(shader, source); gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) || "Aurora shader compilation failed");
      return shader;
    };
    program = gl.createProgram();
    if (!program) throw new Error("Aurora program allocation failed");
    gl.attachShader(program, compile(gl.VERTEX_SHADER, auroraVertex));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, auroraFragment));
    gl.bindAttribLocation(program, 0, "position");
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) || "Aurora program linking failed");
    buffer = gl.createBuffer(); noiseTexture = gl.createTexture();
    if (!buffer || !noiseTexture) throw new Error("Aurora resource allocation failed");
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    let seed = 0x6175726f;
    const noise = new Uint8Array(256 * 256);
    for (let i = 0; i < noise.length; i++) {
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
      noise[i] = (seed >>> 0) & 255;
    }
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, noiseTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, 256, 256, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, noise);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
    const uniforms = Object.fromEntries(["resolution", "displaySize", "time", "activity", "activationAge", "pointer"]
      .map((name) => [name, gl.getUniformLocation(program!, name)]));
    gl.useProgram(program);
    gl.uniform1i(gl.getUniformLocation(program, "noiseMap"), 0);
    let width = 1; let height = 1;
    return {
      resize(nextWidth, nextHeight, quality) {
        if (disposed) return;
        width = Math.max(1, nextWidth); height = Math.max(1, nextHeight);
        const scale = Math.min(qualityScale[quality], 1100 / width, 720 / height);
        canvas.width = Math.max(1, Math.round(width * scale));
        canvas.height = Math.max(1, Math.round(height * scale));
        gl.viewport(0, 0, canvas.width, canvas.height);
      },
      draw(time, activity, activationAge, pointer) {
        if (disposed || gl.isContextLost()) return;
        gl.useProgram(program);
        gl.uniform2f(uniforms.resolution, canvas.width, canvas.height);
        gl.uniform2f(uniforms.displaySize, width, height);
        gl.uniform1f(uniforms.time, time); gl.uniform1f(uniforms.activity, activity);
        gl.uniform1f(uniforms.activationAge, activationAge);
        gl.uniform3f(uniforms.pointer, pointer?.x ?? 0.5, pointer?.y ?? 0.5, pointer?.strength ?? 0);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
      },
      dispose,
    };
  } catch (error) {
    console.warn("Aurora uses its still fallback:", error);
    dispose(); return null;
  }
}
