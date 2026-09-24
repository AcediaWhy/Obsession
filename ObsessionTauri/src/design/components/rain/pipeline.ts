// Двухпроходный WebGL2-пайплайн: мир из плиты и эмиссии → FBO+мипы → композит.
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
import { rainFocusLods } from "./focus";
import { worldPlateFrag, rainPassVert } from "./worldShaders";
import type { RainQualityProfile } from "./quality";
import type { RainWeatherSnapshot } from "./weather";

export type RainWorldFrame = {
  parallaxX: number;
  parallaxY: number;
  elapsed: number;
  weather: RainWeatherSnapshot;
  quality: RainQualityProfile;
};

/** Заглушка для текстур мира до загрузки картинок. */
const BLANK_PIXEL = new Uint8Array([0, 0, 0, 255]);

export class RainPipeline {
  private readonly canvas: HTMLCanvasElement;
  private readonly gl: WebGL2RenderingContext;
  private readonly worldProgram: Gl2Program;
  private readonly compositeProgram: Gl2Program;
  private readonly quad: WebGLVertexArrayObject;
  private readonly worldFbo: Gl2Fbo;
  private readonly waterTexture: WebGLTexture;
  private readonly mistTexture: WebGLTexture;
  private readonly shineTexture: WebGLTexture;
  private readonly plateTexture: WebGLTexture;
  private readonly emissionTexture: WebGLTexture;
  private waterWidth = 0;
  private waterHeight = 0;
  private mistWidth = 0;
  private mistHeight = 0;
  private plateAspect = 16 / 9;
  private width: number;
  private height: number;
  private worldScale: number;
  private mipDepth: number;
  private dropLod = 0;
  private glassLod = 0;
  private mistLod = 0;
  private scatterLod = 0;
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
      const world = createProgram2(gl, rainPassVert, worldPlateFrag);
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
      // Matcap блика капель (drop-shine2 из RainEffect): пока картинка не
      // загружена — прозрачный 1×1, чтобы блика просто не было.
      const shine = gl.createTexture();
      if (!shine) throw new Error("Rain: текстура блика недоступна");
      this.shineTexture = shine;
      gl.bindTexture(gl.TEXTURE_2D, shine);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        1,
        1,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        new Uint8Array([255, 255, 255, 0]),
      );
      gl.bindTexture(gl.TEXTURE_2D, null);
      // Плита мира и карта эмиссии: чёрный 1×1 до загрузки картинок, чтобы
      // первый кадр не читал мусор. Эмиссии нужны мипы — из них берётся ореол.
      const plate = gl.createTexture();
      if (!plate) throw new Error("Rain: текстура подложки недоступна");
      this.plateTexture = plate;
      gl.bindTexture(gl.TEXTURE_2D, plate);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, BLANK_PIXEL);
      const emission = gl.createTexture();
      if (!emission) throw new Error("Rain: текстура эмиссии недоступна");
      this.emissionTexture = emission;
      gl.bindTexture(gl.TEXTURE_2D, emission);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, BLANK_PIXEL);
      gl.generateMipmap(gl.TEXTURE_2D);
      gl.bindTexture(gl.TEXTURE_2D, null);

      this.worldProgram.use();
      this.worldProgram.set1i("u_plate", 4);
      this.worldProgram.set1i("u_emission", 5);
      this.worldProgram.set1f("u_plateAspect", this.plateAspect);

      // Безопасные дефолты композита: валидный кадр до первого world pass.
      this.compositeProgram.use();
      this.compositeProgram.set1i("u_world", 0);
      this.compositeProgram.set1i("u_water", 1);
      this.compositeProgram.set1i("u_mist", 2);
      this.compositeProgram.set1i("u_shine", 3);
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

  /** Пересчитывает уровни детализации при изменении размера FBO.
   *  Правила резкости капель и стекла заданы в rain/focus.ts. */
  private updateLods(): void {
    const lods = rainFocusLods(this.worldFbo.width, this.mipDepth);
    this.dropLod = lods.dropLod;
    this.glassLod = lods.glassLod;
    this.mistLod = lods.mistLod;
    this.scatterLod = lods.scatterLod;
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

  /** Matcap блика капель. Однократная загрузка: картинка не меняется. */
  updateShineTexture(source: TexImageSource): void {
    if (this.destroyed) return;
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + 3);
    gl.bindTexture(gl.TEXTURE_2D, this.shineTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
  }

  /** Плита мира и карта её источников света. Однократная загрузка. Пропорция
   *  плиты нужна шейдеру для cover-fit. */
  updateWorldTextures(plate: TexImageSource, emission: TexImageSource, plateAspect: number): void {
    if (this.destroyed) return;
    const gl = this.gl;
    this.plateAspect = plateAspect > 0 ? plateAspect : 16 / 9;
    gl.activeTexture(gl.TEXTURE0 + 4);
    gl.bindTexture(gl.TEXTURE_2D, this.plateTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, plate);
    gl.activeTexture(gl.TEXTURE0 + 5);
    gl.bindTexture(gl.TEXTURE_2D, this.emissionTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, emission);
    gl.generateMipmap(gl.TEXTURE_2D);
    this.worldProgram.use();
    this.worldProgram.set1f("u_plateAspect", this.plateAspect);
  }

  /** Проход A: мир из плиты и эмиссии (параллакс, погода, время) → FBO, мипы. */
  renderWorld(frame: RainWorldFrame): void {
    if (this.destroyed) return;
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.worldFbo.framebuffer);
    gl.viewport(0, 0, this.worldFbo.width, this.worldFbo.height);
    this.worldProgram.use();
    this.worldProgram.set2f("u_resolution", this.worldFbo.width, this.worldFbo.height);
    this.worldProgram.set2f("u_parallax", frame.parallaxX, frame.parallaxY);
    this.worldProgram.set1f("u_lightning", frame.weather.lightning);
    this.worldProgram.set1f("u_time", frame.elapsed);
    this.worldProgram.set1f("u_activity", frame.weather.activity);
    this.worldProgram.set1f("u_wind", frame.weather.wind);
    gl.activeTexture(gl.TEXTURE0 + 5);
    gl.bindTexture(gl.TEXTURE_2D, this.emissionTexture);
    gl.activeTexture(gl.TEXTURE0 + 4);
    gl.bindTexture(gl.TEXTURE_2D, this.plateTexture);
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
    this.compositeProgram.set1f("u_dropLod", this.dropLod);
    this.compositeProgram.set1f("u_glassLod", this.glassLod);
    this.compositeProgram.set1f("u_mistLod", this.mistLod);
    this.compositeProgram.set1f("u_scatterLod", this.scatterLod);
    this.compositeProgram.set1f("u_time", time);
    gl.activeTexture(gl.TEXTURE0 + 3);
    gl.bindTexture(gl.TEXTURE_2D, this.shineTexture);
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
    gl.deleteTexture(this.shineTexture);
    gl.deleteTexture(this.plateTexture);
    gl.deleteTexture(this.emissionTexture);
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

  /** Жив ли пайплайн для персистентной сессии (см. gl/persistentGlSession). */
  isAlive(): boolean {
    return !this.destroyed && !this.gl.isContextLost();
  }
}
