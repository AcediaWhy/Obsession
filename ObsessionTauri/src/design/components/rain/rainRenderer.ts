// Порт rain-renderer.js — WebGL-рендер: преломляет фон (Fg/Bg) через water map капель.
import { GL } from "./gl";
import { simpleVert, waterFrag } from "./shaders";
import { createCanvas } from "./random";
export interface RainRendererOptions {
  renderShadow: boolean;
  minRefraction: number;
  maxRefraction: number;
  brightness: number;
  alphaMultiply: number;
  alphaSubtract: number;
  parallaxBg: number;
  parallaxFg: number;
}

const defaultOptions: RainRendererOptions = {
  renderShadow: false,
  minRefraction: 256,
  maxRefraction: 512,
  brightness: 1,
  alphaMultiply: 20,
  alphaSubtract: 5,
  parallaxBg: 5,
  parallaxFg: 20,
};

type Tex = { name: string; img: TexImageSource };

export class RainRenderer {
  canvas: HTMLCanvasElement;
  canvasLiquid: HTMLCanvasElement;
  imageShine: HTMLImageElement | null;
  imageFg: TexImageSource;
  imageBg: TexImageSource;
  options: RainRendererOptions;

  gl!: GL;
  width = 0;
  height = 0;
  textures: Tex[] = [];
  glTextures: WebGLTexture[] = [];
  parallaxX = 0;
  parallaxY = 0;

  private destroyed = false;

  // Управляется извне: intensity>1 при активном обходе (не часть оригинала).
  overrideParallax: { x: number; y: number } | null = null;

  constructor(
    canvas: HTMLCanvasElement,
    canvasLiquid: HTMLCanvasElement,
    imageFg: TexImageSource,
    imageBg: TexImageSource,
    imageShine: HTMLImageElement | null = null,
    options: Partial<RainRendererOptions> = {},
  ) {
    this.canvas = canvas;
    this.canvasLiquid = canvasLiquid;
    this.imageShine = imageShine;
    this.imageFg = imageFg;
    this.imageBg = imageBg;
    this.options = { ...defaultOptions, ...options };
    this.init();
  }

  private bgRatio(img: TexImageSource): number {
    const w = (img as HTMLCanvasElement).width;
    const h = (img as HTMLCanvasElement).height;
    return w / h;
  }

  init() {
    this.width = this.canvas.width;
    this.height = this.canvas.height;
    this.gl = new GL(this.canvas, { alpha: false }, simpleVert, waterFrag);
    const gl = this.gl;

    gl.createUniform("2f", "resolution", this.width, this.height);
    gl.createUniform("1f", "textureRatio", this.bgRatio(this.imageBg));
    gl.createUniform("1i", "renderShine", this.imageShine == null ? 0 : 1);
    gl.createUniform("1i", "renderShadow", this.options.renderShadow ? 1 : 0);
    gl.createUniform("1f", "minRefraction", this.options.minRefraction);
    gl.createUniform("1f", "refractionDelta", this.options.maxRefraction - this.options.minRefraction);
    gl.createUniform("1f", "brightness", this.options.brightness);
    gl.createUniform("1f", "alphaMultiply", this.options.alphaMultiply);
    gl.createUniform("1f", "alphaSubtract", this.options.alphaSubtract);
    gl.createUniform("1f", "parallaxBg", this.options.parallaxBg);
    gl.createUniform("1f", "parallaxFg", this.options.parallaxFg);

    this.glTextures.push(gl.createTexture(null, 0));

    this.textures = [
      { name: "textureShine", img: this.imageShine == null ? createCanvas(2, 2) : this.imageShine },
      { name: "textureFg", img: this.imageFg },
      { name: "textureBg", img: this.imageBg },
    ];

    this.textures.forEach((texture, i) => {
      this.glTextures.push(gl.createTexture(texture.img, i + 1));
      gl.createUniform("1i", texture.name, i + 1);
    });
  }

  draw() {
    if (this.destroyed) return;
    this.gl.useProgram(this.gl.program);
    this.gl.createUniform("2f", "parallax", this.parallaxX, this.parallaxY);
    this.gl.draw();
  }

  updateTextures() {
    this.textures.forEach((texture, i) => {
      this.gl.activeTexture(i + 1);
      this.gl.updateTexture(texture.img);
    });
  }

  /** Подгоняет WebGL-вьюпорт и uniform resolution под новый размер холста.
   *  Без этого при ресайзе окна (напр. разворот на весь экран) вьюпорт остаётся
   *  на размере создания контекста, и сцена рисуется в старом прямоугольнике. */
  resize(width: number, height: number) {
    if (this.destroyed || (width === this.width && height === this.height)) return;
    this.width = width;
    this.height = height;
    this.gl.useProgram(this.gl.program);
    this.gl.gl.viewport(0, 0, width, height);
    this.gl.createUniform("2f", "resolution", width, height);
  }

  updateTexture() {
    if (this.destroyed) return;
    this.gl.activeTexture(0);
    this.gl.updateTexture(this.canvasLiquid);
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    // Освобождаем GPU-ресурсы: текстуры + буферы/программу/контекст.
    const gl = this.gl?.gl;
    if (gl) {
      for (const tex of this.glTextures) gl.deleteTexture(tex);
    }
    this.glTextures = [];
    this.gl?.destroy();
    // Рвём ссылки на большие канвасы (fg/bg) и water map, чтобы GC их собрал.
    this.textures = [];
    this.imageShine = null;
    this.imageFg = null as unknown as TexImageSource;
    this.imageBg = null as unknown as TexImageSource;
  }
}
