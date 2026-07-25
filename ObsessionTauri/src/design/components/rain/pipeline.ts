// Двухпроходный WebGL2-пайплайн: видео-мир → FBO+мипы → композит.
// Владеет контекстом, программами, FBO и текстурой водной карты; сцена
// (RainHybridScene) гоняет его из фаз render-цикла.
import {
  Gl2Program,
  createFbo,
  createFullscreenTriangle,
  createProgram2,
  deleteFbo,
  generateMips,
  getContext2,
  resizeFbo,
  type Gl2Fbo,
} from "./gl2";
import { compositeFrag, compositeVert } from "./compositeShaders";
import { worldVideoFrag, rainPassVert } from "./worldShaders";
import type { RainQualityProfile } from "./quality";
import type { RainWeatherSnapshot } from "./weather";

export type RainWorldFrame = {
  parallaxX: number;
  parallaxY: number;
  elapsed: number;
  weather: RainWeatherSnapshot;
  quality: RainQualityProfile;
};

export class RainPipeline {
  private readonly canvas: HTMLCanvasElement;
  private readonly gl: WebGL2RenderingContext;
  private readonly worldProgram: Gl2Program;
  private readonly compositeProgram: Gl2Program;
  private readonly quad: WebGLVertexArrayObject;
  private readonly worldFbo: Gl2Fbo;
  private readonly waterTexture: WebGLTexture;
  private readonly mistTexture: WebGLTexture;
  private readonly videoTexture: WebGLTexture;
  private waterWidth = 0;
  private waterHeight = 0;
  private mistWidth = 0;
  private mistHeight = 0;
  private videoWidth = 0;
  private videoHeight = 0;
  private lastVideoTime = -1;
  private width: number;
  private height: number;
  private worldScale: number;
  private mipDepth: number;
  private fgLod = 0;
  private bgLod = 0;
  private destroyed = false;

  constructor(canvas: HTMLCanvasElement, backingWidth: number, backingHeight: number, quality: RainQualityProfile) {
    this.canvas = canvas;
    this.width = Math.max(1, Math.round(backingWidth));
    this.height = Math.max(1, Math.round(backingHeight));
    this.worldScale = quality.worldScale;
    this.mipDepth = quality.mipDepth;
    const gl = getContext2(canvas, { alpha: false, antialias: false });
    if (!gl) throw new Error("Rain: WebGL2 недоступен");
    this.gl = gl;
    canvas.width = this.width;
    canvas.height = this.height;

    try {
      const world = createProgram2(gl, rainPassVert, worldVideoFrag);
      const composite = createProgram2(gl, compositeVert, compositeFrag);
      if (!world || !composite) {
        if (world) gl.deleteProgram(world);
        if (composite) gl.deleteProgram(composite);
        throw new Error("Rain: не удалось собрать GL2-программы");
      }
      this.worldProgram = new Gl2Program(gl, world);
      this.compositeProgram = new Gl2Program(gl, composite);
      this.quad = createFullscreenTriangle(gl);
      this.worldFbo = createFbo(
        gl,
        this.width * this.worldScale,
        this.height * this.worldScale,
      );
      this.updateLods();
      // Текстура водной карты: RGBA, линейная, без мипов.
      const water = gl.createTexture();
      if (!water) throw new Error("Rain: текстура water map недоступна");
      this.waterTexture = water;
      gl.bindTexture(gl.TEXTURE_2D, water);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.bindTexture(gl.TEXTURE_2D, null);
      // Текстура конденсата: крошечная сетка, линейный апскейл сглаживает.
      const mist = gl.createTexture();
      if (!mist) throw new Error("Rain: текстура mist map недоступна");
      this.mistTexture = mist;
      gl.bindTexture(gl.TEXTURE_2D, mist);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.bindTexture(gl.TEXTURE_2D, null);

      // Текстура кадра видео-мира: чёрный 1×1 до первого кадра, чтобы
      // композит не читал мусор, пока видео грузится.
      const videoTex = gl.createTexture();
      if (!videoTex) throw new Error("Rain: текстура видео недоступна");
      this.videoTexture = videoTex;
      gl.bindTexture(gl.TEXTURE_2D, videoTex);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([4, 6, 10, 255]));
      gl.bindTexture(gl.TEXTURE_2D, null);

      this.worldProgram.use();
      this.worldProgram.set1i("u_video", 3);

      // Безопасные дефолты композита: валидный кадр до первого world pass.
      this.compositeProgram.use();
      this.compositeProgram.set1i("u_world", 0);
      this.compositeProgram.set1i("u_water", 1);
      this.compositeProgram.set1i("u_mist", 2);
      this.compositeProgram.set2f("u_resolution", this.width, this.height);
    } catch (error) {
      gl.getExtension("WEBGL_lose_context")?.loseContext();
      throw error;
    }
  }

  resize(width: number, height: number, quality: RainQualityProfile): void {
    if (this.destroyed) return;
    const w = Math.max(1, Math.round(width));
    const h = Math.max(1, Math.round(height));
    this.worldScale = quality.worldScale;
    this.mipDepth = quality.mipDepth;
    if (w !== this.width || h !== this.height) {
      this.width = w;
      this.height = h;
      this.canvas.width = w;
      this.canvas.height = h;
    }
    resizeFbo(this.gl, this.worldFbo, w * this.worldScale, h * this.worldScale);
    this.updateLods();
  }

  /** Мипы: fg 96px — нутро капли как в демо codrops (мягкая светящаяся
   *  линза); bg почти резкий (реф 1024px) — наш видеофон сам по себе мягкий,
   *  дополнительное мыло 384px превращало его в кашу. Пересчёт при ресайзе FBO. */
  private updateLods(): void {
    const w = Math.max(1, this.worldFbo.width);
    this.fgLod = Math.min(this.mipDepth, Math.max(0, Math.log2(w / 96)));
    this.bgLod = Math.min(this.mipDepth, Math.max(0, Math.log2(w / 1024)));
  }

  updateWaterTexture(source: HTMLCanvasElement): void {
    if (this.destroyed) return;
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + 1);
    gl.bindTexture(gl.TEXTURE_2D, this.waterTexture);
    if (source.width !== this.waterWidth || source.height !== this.waterHeight) {
      this.waterWidth = source.width;
      this.waterHeight = source.height;
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, gl.RGBA, gl.UNSIGNED_BYTE, source);
    }
  }

  updateMistTexture(source: HTMLCanvasElement): void {
    if (this.destroyed) return;
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + 2);
    gl.bindTexture(gl.TEXTURE_2D, this.mistTexture);
    if (source.width !== this.mistWidth || source.height !== this.mistHeight) {
      this.mistWidth = source.width;
      this.mistHeight = source.height;
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, gl.RGBA, gl.UNSIGNED_BYTE, source);
    }
  }

  /** Статичный фото-мир (режим codrops-демо): однократная загрузка кадра. */
  updateWorldImage(image: HTMLImageElement): void {
    if (this.destroyed) return;
    if (image.naturalWidth === 0) return;
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + 3);
    gl.bindTexture(gl.TEXTURE_2D, this.videoTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
    this.videoWidth = image.naturalWidth;
    this.videoHeight = image.naturalHeight;
    this.lastVideoTime = -1;
  }

  /** Загрузка свежего кадра видео в текстуру мира (пропускает повторы). */
  updateVideoTexture(video: HTMLVideoElement): void {
    if (this.destroyed) return;
    if (video.readyState < 2 || video.videoWidth === 0) return;
    if (
      video.currentTime === this.lastVideoTime &&
      video.videoWidth === this.videoWidth
    ) {
      return;
    }
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + 3);
    gl.bindTexture(gl.TEXTURE_2D, this.videoTexture);
    if (video.videoWidth !== this.videoWidth || video.videoHeight !== this.videoHeight) {
      this.videoWidth = video.videoWidth;
      this.videoHeight = video.videoHeight;
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video);
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, gl.RGBA, gl.UNSIGNED_BYTE, video);
    }
    this.lastVideoTime = video.currentTime;
  }

  /** Проход A: кадр видео (cover-fit, параллакс, зарницы) → FBO, затем мипы. */
  renderWorld(frame: RainWorldFrame): void {
    if (this.destroyed) return;
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.worldFbo.framebuffer);
    gl.viewport(0, 0, this.worldFbo.width, this.worldFbo.height);
    this.worldProgram.use();
    this.worldProgram.set2f("u_resolution", this.worldFbo.width, this.worldFbo.height);
    this.worldProgram.set1f(
      "u_videoAspect",
      this.videoWidth > 0 ? this.videoWidth / Math.max(1, this.videoHeight) : 16 / 9,
    );
    this.worldProgram.set2f("u_parallax", frame.parallaxX, frame.parallaxY);
    this.worldProgram.set1f("u_lightning", frame.weather.lightning);
    gl.activeTexture(gl.TEXTURE0 + 3);
    gl.bindTexture(gl.TEXTURE_2D, this.videoTexture);
    gl.bindVertexArray(this.quad);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    generateMips(gl, this.worldFbo);
  }

  /** Проход B: композит на экран. Молния подсвечивает интерьер, время
   *  крутит плёночное зерно. */
  renderComposite(lightning = 0, time = 0): void {
    if (this.destroyed) return;
    const gl = this.gl;
    gl.viewport(0, 0, this.width, this.height);
    this.compositeProgram.use();
    this.compositeProgram.set2f("u_resolution", this.width, this.height);
    this.compositeProgram.set1f("u_lightning", lightning);
    this.compositeProgram.set1f("u_fgLod", this.fgLod);
    this.compositeProgram.set1f("u_bgLod", this.bgLod);
    this.compositeProgram.set1f("u_time", time);
    gl.activeTexture(gl.TEXTURE0 + 2);
    gl.bindTexture(gl.TEXTURE_2D, this.mistTexture);
    gl.activeTexture(gl.TEXTURE0 + 1);
    gl.bindTexture(gl.TEXTURE_2D, this.waterTexture);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.worldFbo.texture);
    gl.bindVertexArray(this.quad);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    const gl = this.gl;
    gl.deleteTexture(this.waterTexture);
    gl.deleteTexture(this.mistTexture);
    gl.deleteTexture(this.videoTexture);
    deleteFbo(gl, this.worldFbo);
    gl.deleteVertexArray(this.quad);
    this.worldProgram.dispose();
    this.compositeProgram.dispose();
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    this.canvas.width = 0;
    this.canvas.height = 0;
  }

  /** После context loss: ресурсы уже мертвы, только отпускаем ссылки. */
  abandonAfterContextLoss(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.worldProgram.dispose();
    this.compositeProgram.dispose();
  }
}
