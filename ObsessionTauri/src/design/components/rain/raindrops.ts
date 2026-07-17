// Порт raindrops.js из codrops/RainEffect — симуляция капель, пишущая «water map».
// Максимально дословно (структура, формулы, константы сохранены), добавлены типы
// и явные lifecycle-методы для внешнего frame pipeline.
import { random, chance, times, createCanvas } from "./random";

const dropSize = 64;

interface Drop {
  x: number;
  y: number;
  r: number;
  spreadX: number;
  spreadY: number;
  momentum: number;
  momentumX: number;
  lastSpawn: number;
  nextSpawn: number;
  parent: Drop | null;
  isNew: boolean;
  killed: boolean;
  shrink: number;
}

function makeDrop(o: Partial<Drop>): Drop {
  return {
    x: 0,
    y: 0,
    r: 0,
    spreadX: 0,
    spreadY: 0,
    momentum: 0,
    momentumX: 0,
    lastSpawn: 0,
    nextSpawn: 0,
    parent: null,
    isNew: true,
    killed: false,
    shrink: 0,
    ...o,
  };
}

export interface RaindropsOptions {
  minR: number;
  maxR: number;
  maxDrops: number;
  rainChance: number;
  rainLimit: number;
  dropletsRate: number;
  dropletsSize: [number, number];
  dropletsCleaningRadiusMultiplier: number;
  raining: boolean;
  globalTimeScale: number;
  trailRate: number;
  autoShrink: boolean;
  spawnArea: [number, number];
  trailScaleRange: [number, number];
  collisionRadius: number;
  collisionRadiusIncrease: number;
  dropFallMultiplier: number;
  collisionBoostMultiplier: number;
  collisionBoost: number;
}

const defaultOptions: RaindropsOptions = {
  minR: 10,
  maxR: 40,
  maxDrops: 900,
  rainChance: 0.3,
  rainLimit: 3,
  dropletsRate: 50,
  dropletsSize: [2, 4],
  dropletsCleaningRadiusMultiplier: 0.43,
  raining: true,
  globalTimeScale: 1,
  trailRate: 1,
  autoShrink: true,
  spawnArea: [-0.1, 0.95],
  trailScaleRange: [0.2, 0.5],
  collisionRadius: 0.65,
  collisionRadiusIncrease: 0.01,
  dropFallMultiplier: 1,
  collisionBoostMultiplier: 0.05,
  collisionBoost: 1,
};

export class Raindrops {
  width: number;
  height: number;
  scale: number;
  dropAlpha: HTMLImageElement;
  dropColor: HTMLImageElement;
  options: RaindropsOptions;

  canvas!: HTMLCanvasElement;
  ctx!: CanvasRenderingContext2D;
  droplets!: HTMLCanvasElement;
  dropletsCtx!: CanvasRenderingContext2D;
  dropletsPixelDensity = 1;
  dropletsCounter = 0;
  drops: Drop[] = [];
  dropsGfx: HTMLCanvasElement[] = [];
  clearDropletsGfx!: HTMLCanvasElement;
  textureCleaningIterations = 0;

  private destroyed = false;

  constructor(
    width: number,
    height: number,
    scale: number,
    dropAlpha: HTMLImageElement,
    dropColor: HTMLImageElement,
    options: Partial<RaindropsOptions> = {},
  ) {
    this.width = width;
    this.height = height;
    this.scale = scale;
    this.dropAlpha = dropAlpha;
    this.dropColor = dropColor;
    this.options = { ...defaultOptions, ...options };
    this.init();
  }

  get deltaR() {
    return this.options.maxR - this.options.minR;
  }
  get area() {
    return (this.width * this.height) / this.scale;
  }
  get areaMultiplier() {
    return Math.sqrt(this.area / (1024 * 768));
  }

  init() {
    this.canvas = createCanvas(this.width, this.height);
    this.ctx = this.canvas.getContext("2d")!;
    this.droplets = createCanvas(
      this.width * this.dropletsPixelDensity,
      this.height * this.dropletsPixelDensity,
    );
    this.dropletsCtx = this.droplets.getContext("2d")!;
    this.drops = [];
    this.dropsGfx = [];
    this.renderDropsGfx();
  }

  /** Подгоняет размер water-map под новый размер окна. Ресайзим канвасы
   *  in-place (this.canvas держит RainRenderer как canvasLiquid — пересоздание
   *  порвало бы ссылку). Капли живут в нормализованных к scale координатах,
   *  поэтому переживают ресайз; меняются только границы спавна/буферы. */
  resize(width: number, height: number) {
    if (this.destroyed || (width === this.width && height === this.height)) return;
    this.width = width;
    this.height = height;
    this.canvas.width = width;
    this.canvas.height = height;
    this.droplets.width = width * this.dropletsPixelDensity;
    this.droplets.height = height * this.dropletsPixelDensity;
    // Установка .width/.height уже очищает оба буфера — доп. clear не нужен.
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    // Освобождаем оффскрин-канвасы: ~255 спрайтов капель (dropSize²), маску
    // очистки и два полноэкранных буфера. Обнуляем размеры — так WebView2
    // отпускает backing store сразу, не дожидаясь GC.
    const release = (c: HTMLCanvasElement | undefined | null) => {
      if (c) {
        c.width = 0;
        c.height = 0;
      }
    };
    for (const g of this.dropsGfx) release(g);
    this.dropsGfx = [];
    release(this.clearDropletsGfx);
    release(this.canvas);
    release(this.droplets);
    this.drops = [];
  }

  drawDroplet(x: number, y: number, r: number) {
    this.drawDrop(
      this.dropletsCtx,
      makeDrop({
        x: x * this.dropletsPixelDensity,
        y: y * this.dropletsPixelDensity,
        r: r * this.dropletsPixelDensity,
      }),
    );
  }

  renderDropsGfx() {
    const dropBuffer = createCanvas(dropSize, dropSize);
    const dropBufferCtx = dropBuffer.getContext("2d")!;
    this.dropsGfx = Array.apply(null, Array(255) as unknown[]).map((_cur, i) => {
      const drop = createCanvas(dropSize, dropSize);
      const dropCtx = drop.getContext("2d")!;

      dropBufferCtx.clearRect(0, 0, dropSize, dropSize);

      // color
      dropBufferCtx.globalCompositeOperation = "source-over";
      dropBufferCtx.drawImage(this.dropColor, 0, 0, dropSize, dropSize);

      // blue overlay, for depth
      dropBufferCtx.globalCompositeOperation = "screen";
      dropBufferCtx.fillStyle = "rgba(0,0," + i + ",1)";
      dropBufferCtx.fillRect(0, 0, dropSize, dropSize);

      // alpha
      dropCtx.globalCompositeOperation = "source-over";
      dropCtx.drawImage(this.dropAlpha, 0, 0, dropSize, dropSize);

      dropCtx.globalCompositeOperation = "source-in";
      dropCtx.drawImage(dropBuffer, 0, 0, dropSize, dropSize);
      return drop;
    });

    this.clearDropletsGfx = createCanvas(128, 128);
    const clearDropletsCtx = this.clearDropletsGfx.getContext("2d")!;
    clearDropletsCtx.fillStyle = "#000";
    clearDropletsCtx.beginPath();
    clearDropletsCtx.arc(64, 64, 64, 0, Math.PI * 2);
    clearDropletsCtx.fill();
  }

  drawDrop(ctx: CanvasRenderingContext2D, drop: Drop) {
    if (this.dropsGfx.length > 0) {
      const { x, y, r, spreadX, spreadY } = drop;
      const scaleX = 1;
      const scaleY = 1.5;

      let d = Math.max(0, Math.min(1, ((r - this.options.minR) / this.deltaR) * 0.9));
      d *= 1 / ((drop.spreadX + drop.spreadY) * 0.5 + 1);

      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";

      d = Math.floor(d * (this.dropsGfx.length - 1));
      ctx.drawImage(
        this.dropsGfx[d],
        (x - r * scaleX * (spreadX + 1)) * this.scale,
        (y - r * scaleY * (spreadY + 1)) * this.scale,
        r * 2 * scaleX * (spreadX + 1) * this.scale,
        r * 2 * scaleY * (spreadY + 1) * this.scale,
      );
    }
  }

  clearDroplets(x: number, y: number, r = 30) {
    const ctx = this.dropletsCtx;
    ctx.globalCompositeOperation = "destination-out";
    ctx.drawImage(
      this.clearDropletsGfx,
      (x - r) * this.dropletsPixelDensity * this.scale,
      (y - r) * this.dropletsPixelDensity * this.scale,
      r * 2 * this.dropletsPixelDensity * this.scale,
      r * 2 * this.dropletsPixelDensity * this.scale * 1.5,
    );
  }

  clearCanvas() {
    this.ctx.clearRect(0, 0, this.width, this.height);
  }

  createDrop(options: Partial<Drop>): Drop | null {
    if (this.drops.length >= this.options.maxDrops * this.areaMultiplier) return null;
    return makeDrop(options);
  }

  addDrop(drop: Drop | null): boolean {
    if (this.drops.length >= this.options.maxDrops * this.areaMultiplier || drop == null) return false;
    this.drops.push(drop);
    return true;
  }

  updateRain(timeScale: number): Drop[] {
    const rainDrops: Drop[] = [];
    if (this.options.raining) {
      const limit = this.options.rainLimit * timeScale * this.areaMultiplier;
      let count = 0;
      while (chance(this.options.rainChance * timeScale * this.areaMultiplier) && count < limit) {
        count++;
        const r = random(this.options.minR, this.options.maxR, (n) => Math.pow(n, 3));
        const rainDrop = this.createDrop({
          x: random(this.width / this.scale),
          y: random(
            (this.height / this.scale) * this.options.spawnArea[0],
            (this.height / this.scale) * this.options.spawnArea[1],
          ),
          r,
          momentum: 1 + (r - this.options.minR) * 0.1 + random(2),
          spreadX: 1.5,
          spreadY: 1.5,
        });
        if (rainDrop != null) rainDrops.push(rainDrop);
      }
    }
    return rainDrops;
  }

  updateDroplets(timeScale: number) {
    if (this.textureCleaningIterations > 0) {
      this.textureCleaningIterations -= 1 * timeScale;
      this.dropletsCtx.globalCompositeOperation = "destination-out";
      this.dropletsCtx.fillStyle = "rgba(0,0,0," + 0.05 * timeScale + ")";
      this.dropletsCtx.fillRect(
        0,
        0,
        this.width * this.dropletsPixelDensity,
        this.height * this.dropletsPixelDensity,
      );
    }
    if (this.options.raining) {
      this.dropletsCounter += this.options.dropletsRate * timeScale * this.areaMultiplier;
      times(this.dropletsCounter, () => {
        this.dropletsCounter--;
        this.drawDroplet(
          random(this.width / this.scale),
          random(this.height / this.scale),
          random(this.options.dropletsSize[0], this.options.dropletsSize[1], (n) => n * n),
        );
      });
    }
    this.ctx.drawImage(this.droplets, 0, 0, this.width, this.height);
  }

  updateDrops(timeScale: number) {
    let newDrops: Drop[] = [];

    this.updateDroplets(timeScale);
    const rainDrops = this.updateRain(timeScale);
    newDrops = newDrops.concat(rainDrops);

    this.drops.sort((a, b) => {
      const va = a.y * (this.width / this.scale) + a.x;
      const vb = b.y * (this.width / this.scale) + b.x;
      return va > vb ? 1 : va == vb ? 0 : -1;
    });

    this.drops.forEach((drop, i) => {
      if (!drop.killed) {
        // update gravity (chance of drops "creeping down")
        if (
          chance(
            (drop.r - this.options.minR * this.options.dropFallMultiplier) *
              (0.1 / this.deltaR) *
              timeScale,
          )
        ) {
          drop.momentum += random((drop.r / this.options.maxR) * 4);
        }
        // clean small drops
        if (this.options.autoShrink && drop.r <= this.options.minR && chance(0.05 * timeScale)) {
          drop.shrink += 0.01;
        }
        // update shrinkage
        drop.r -= drop.shrink * timeScale;
        if (drop.r <= 0) drop.killed = true;

        // update trails
        if (this.options.raining) {
          drop.lastSpawn += drop.momentum * timeScale * this.options.trailRate;
          if (drop.lastSpawn > drop.nextSpawn) {
            const trailDrop = this.createDrop({
              x: drop.x + random(-drop.r, drop.r) * 0.1,
              y: drop.y - drop.r * 0.01,
              r: drop.r * random(this.options.trailScaleRange[0], this.options.trailScaleRange[1]),
              spreadY: drop.momentum * 0.1,
              parent: drop,
            });
            if (trailDrop != null) {
              newDrops.push(trailDrop);
              drop.r *= Math.pow(0.97, timeScale);
              drop.lastSpawn = 0;
              drop.nextSpawn =
                random(this.options.minR, this.options.maxR) -
                drop.momentum * 2 * this.options.trailRate +
                (this.options.maxR - drop.r);
            }
          }
        }

        // normalize spread
        drop.spreadX *= Math.pow(0.4, timeScale);
        drop.spreadY *= Math.pow(0.7, timeScale);

        // update position
        const moved = drop.momentum > 0;
        if (moved && !drop.killed) {
          drop.y += drop.momentum * this.options.globalTimeScale;
          drop.x += drop.momentumX * this.options.globalTimeScale;
          if (drop.y > this.height / this.scale + drop.r) drop.killed = true;
        }

        // collision
        const checkCollision = (moved || drop.isNew) && !drop.killed;
        drop.isNew = false;

        if (checkCollision) {
          this.drops.slice(i + 1, i + 70).forEach((drop2) => {
            if (
              drop != drop2 &&
              drop.r > drop2.r &&
              drop.parent != drop2 &&
              drop2.parent != drop &&
              !drop2.killed
            ) {
              const dx = drop2.x - drop.x;
              const dy = drop2.y - drop.y;
              const dd = Math.sqrt(dx * dx + dy * dy);
              if (
                dd <
                (drop.r + drop2.r) *
                  (this.options.collisionRadius +
                    drop.momentum * this.options.collisionRadiusIncrease * timeScale)
              ) {
                const pi = Math.PI;
                const r1 = drop.r;
                const r2 = drop2.r;
                const a1 = pi * (r1 * r1);
                const a2 = pi * (r2 * r2);
                let targetR = Math.sqrt((a1 + a2 * 0.8) / pi);
                if (targetR > this.options.maxR) targetR = this.options.maxR;
                drop.r = targetR;
                drop.momentumX += dx * 0.1;
                drop.spreadX = 0;
                drop.spreadY = 0;
                drop2.killed = true;
                drop.momentum = Math.max(
                  drop2.momentum,
                  Math.min(
                    40,
                    drop.momentum +
                      targetR * this.options.collisionBoostMultiplier +
                      this.options.collisionBoost,
                  ),
                );
              }
            }
          });
        }

        // slowdown momentum
        drop.momentum -= Math.max(1, this.options.minR * 0.5 - drop.momentum) * 0.1 * timeScale;
        if (drop.momentum < 0) drop.momentum = 0;
        drop.momentumX *= Math.pow(0.7, timeScale);

        if (!drop.killed) {
          newDrops.push(drop);
          if (moved && this.options.dropletsRate > 0)
            this.clearDroplets(drop.x, drop.y, drop.r * this.options.dropletsCleaningRadiusMultiplier);
          this.drawDrop(this.ctx, drop);
        }
      }
    });

    this.drops = newDrops;
  }

  step(dt: number) {
    if (this.destroyed) return;
    this.clearCanvas();
    // timeScale — от реального dt (хелпер уже клампит его maxDt), а не от
    // «идеального» кадра: прежний потолок 1.1 на каждом пропущенном кадре
    // (deltaT≈33 мс → timeScale 2.0 → кламп 1.1) вёл физику в полскорости —
    // дождь буквально замедлялся. Потолок 2.0 оставлен как страховка формул
    // (Math.pow(0.x, timeScale), коллизии) от взрыва после фриза.
    let timeScale = dt * 60;
    if (timeScale > 2) timeScale = 2;
    timeScale *= this.options.globalTimeScale;
    this.updateDrops(timeScale);
  }
}
