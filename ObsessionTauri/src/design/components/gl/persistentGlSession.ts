// Сессия повторно использует canvas и GL-пайплайн при переключении тем.
// acquire() создаёт их заново после invalidate(), release() или повреждения.

export type GlSession<TPipeline> = {
  readonly canvas: HTMLCanvasElement;
  readonly pipeline: TPipeline;
};

export class PersistentGlSession<TPipeline> {
  private current: { canvas: HTMLCanvasElement; pipeline: TPipeline } | null = null;

  constructor(
    private readonly createPipeline: (canvas: HTMLCanvasElement) => TPipeline,
    private readonly isBroken: (pipeline: TPipeline) => boolean,
    // Фабрика передаётся явно для запуска тестов без DOM.
    private readonly createCanvas: () => HTMLCanvasElement = () => document.createElement("canvas"),
  ) {}

  acquire(): GlSession<TPipeline> {
    if (this.current && this.isBroken(this.current.pipeline)) {
      this.current = null;
    }
    if (!this.current) {
      const canvas = this.createCanvas();
      this.current = { canvas, pipeline: this.createPipeline(canvas) };
    }
    return this.current;
  }

  /** Принудительно забыть сессию — из обработчика webglcontextlost. */
  invalidate(): void {
    this.current = null;
  }

  /** Уничтожает пайплайн и контекст при выгрузке скрытого окна.
   *  Следующий acquire() создаст новую сессию. */
  release(): void {
    const s = this.current;
    this.current = null;
    (s?.pipeline as { destroy?: () => void } | undefined)?.destroy?.();
  }

  get alive(): boolean {
    return this.current !== null;
  }
}
