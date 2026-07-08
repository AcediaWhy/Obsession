// Рендерер depth-parallax: рисует фуллскрин-квад с фото, смещённым по карте
// глубины. Переиспользует WebGL-обвязку темы Rain (GL из ../rain/gl) —
// программа + квад + текстуры + uniform'ы уже там, дублировать незачем.
import { GL } from "../rain/gl";
import { simpleVert } from "../rain/shaders";
import { depthFrag } from "./depthShaders";

export class DepthRenderer {
  private gl: GL;
  private ratio: number;

  parallaxX = 0;
  parallaxY = 0;
  scale: number;
  focus: number;
  invert: boolean;

  constructor(
    canvas: HTMLCanvasElement,
    photo: HTMLImageElement,
    // Карта глубины: настоящий PNG или синтезированный canvas-градиент.
    depth: TexImageSource,
    options: { scale?: number; focus?: number; invert?: boolean } = {},
  ) {
    this.scale = options.scale ?? 42;
    this.focus = options.focus ?? 0.5;
    this.invert = options.invert ?? false;
    this.ratio = (photo.naturalWidth || 1) / (photo.naturalHeight || 1);

    // Может бросить, если WebGL недоступен — ловим на стороне компонента.
    this.gl = new GL(canvas, { alpha: false }, simpleVert, depthFrag);

    // Юниты текстур: 0 — фото, 1 — карта глубины.
    this.gl.createTexture(photo, 0);
    this.gl.createTexture(depth, 1);
    this.gl.createUniform("1i", "photo", 0);
    this.gl.createUniform("1i", "depth", 1);
    this.gl.createUniform("1f", "textureRatio", this.ratio);
    this.gl.createUniform("1f", "invert", this.invert ? 1 : 0);

    this.resize(canvas.width, canvas.height);
  }

  resize(w: number, h: number) {
    this.gl.gl.viewport(0, 0, w, h);
    this.gl.createUniform("2f", "resolution", w, h);
  }

  render(time: number) {
    this.gl.createUniform("2f", "parallax", this.parallaxX, this.parallaxY);
    this.gl.createUniform("1f", "scale", this.scale);
    this.gl.createUniform("1f", "focus", this.focus);
    this.gl.createUniform("1f", "time", time);
    this.gl.draw();
  }
}
