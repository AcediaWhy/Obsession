// Симуляция воды на стекле: порт codrops/RainEffect (raindrops.ts, спрайты из
// drop-color/drop-alpha) + наша симуляция конденсата (mistSim). Наружу отдаёт
// два canvas-источника текстур: waterMap (капли+микрокапли, RG=фото-рефракция,
// B=толщина, A=маска) и mistMap (уровень запотевания). Обёртка сохраняет
// прежний публичный API (step/resize/setQuality/stats/destroy).
import { RainMistSim } from "./mistSim";
import type { RainQualityProfile } from "./quality";
import { Raindrops } from "./raindrops";
import type { RainWeatherSnapshot } from "./weather";

/** Спрайты капли из codrops (фото-рефракционная карта + маска-слеза). */
export type RainDropSprites = {
  dropColor: CanvasImageSource;
  dropAlpha: CanvasImageSource;
};

function createCanvas(width: number, height: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}

export class RainSimulation {
  readonly waterMap: HTMLCanvasElement;
  /** Карта конденсата (уровень в R): маленькая сетка, композит тянет её линейно. */
  readonly mistMap: HTMLCanvasElement;
  private readonly raindrops: Raindrops;
  private readonly mist: RainMistSim;
  private readonly mistCtx: CanvasRenderingContext2D;
  private mistImage: ImageData;
  private destroyed = false;

  constructor(
    width: number,
    height: number,
    scale: number,
    quality: RainQualityProfile,
    sprites: RainDropSprites,
  ) {
    this.raindrops = new Raindrops(
      Math.max(1, width),
      Math.max(1, height),
      Math.max(0.1, scale),
      sprites.dropAlpha,
      sprites.dropColor,
      {
        maxDrops: quality.maxDrops,
        // Параметры флагманского демо codrops (слайд rain/storm).
        minR: 20,
        maxR: 45,
        collisionRadius: 0.45,
        collisionRadiusIncrease: 0.0002,
        dropletsCleaningRadiusMultiplier: 0.28,
        dropletsSize: [3, 5.5],
        trailScaleRange: [0.25, 0.35],
      },
    );
    this.waterMap = this.raindrops.canvas;

    const mistCols = quality.mistGrid;
    const mistRows = Math.max(2, Math.round(mistCols * (height / Math.max(1, width))));
    this.mist = new RainMistSim(mistCols, mistRows);
    this.mistMap = createCanvas(mistCols, mistRows);
    const mistCtx = this.mistMap.getContext("2d");
    if (!mistCtx) throw new Error("Rain: Canvas2D mist map недоступен");
    this.mistCtx = mistCtx;
    this.mistImage = mistCtx.createImageData(mistCols, mistRows);

    // Прогрев: стекло при маунте уже в микрокаплях и первых дорожках, а не
    // девственно чистое (важно и для reduce-motion — там будет один кадр).
    for (let i = 0; i < 90; i += 1) {
      this.raindrops.step(1 / 60);
      this.mist.step(1 / 60, 0.4, this.raindrops.wipes);
    }
    this.mist.writeTo(this.mistImage.data);
    this.mistCtx.putImageData(this.mistImage, 0, 0);
  }

  resize(width: number, height: number, scale: number): void {
    if (this.destroyed) return;
    this.raindrops.scale = Math.max(0.1, scale);
    this.raindrops.resize(Math.max(1, width), Math.max(1, height));
    const { cols } = this.mist.size;
    this.resizeMist(cols, Math.max(2, Math.round(cols * (height / Math.max(1, width)))));
  }

  setQuality(quality: RainQualityProfile): void {
    this.raindrops.options.maxDrops = quality.maxDrops;
    const rows = Math.max(
      2,
      Math.round(quality.mistGrid * (this.waterMap.height / Math.max(1, this.waterMap.width))),
    );
    this.resizeMist(quality.mistGrid, rows);
  }

  step(dt: number, weather: RainWeatherSnapshot): void {
    if (this.destroyed) return;
    const elapsed = Math.max(0, Math.min(dt, 0.1));
    // Шторм-каплинг по пресетам codrops: rain (покой) → storm (обход активен).
    const a = weather.activity;
    const options = this.raindrops.options;
    options.rainChance = 0.15 + 0.25 * a;
    options.rainLimit = 2 + 4 * a;
    options.dropletsRate = 15 + 65 * a;
    options.maxR = 45 + 10 * a;
    options.trailRate = (0.8 + 1.7 * a) * Math.max(0.5, Math.min(1.3, weather.trailRate));
    options.globalTimeScale = weather.rainSpeed;

    this.raindrops.step(elapsed);
    this.mist.step(elapsed, weather.activity, this.raindrops.wipes);
    this.mist.writeTo(this.mistImage.data);
    this.mistCtx.putImageData(this.mistImage, 0, 0);
  }

  stats(): { drops: number } {
    return { drops: this.raindrops.drops.length };
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.raindrops.destroy();
    this.mistMap.width = 0;
    this.mistMap.height = 0;
  }

  private resizeMist(cols: number, rows: number): void {
    this.mist.resize(cols, rows);
    const size = this.mist.size;
    if (this.mistMap.width !== size.cols || this.mistMap.height !== size.rows) {
      this.mistMap.width = size.cols;
      this.mistMap.height = size.rows;
      this.mistImage = this.mistCtx.createImageData(size.cols, size.rows);
    }
  }
}
