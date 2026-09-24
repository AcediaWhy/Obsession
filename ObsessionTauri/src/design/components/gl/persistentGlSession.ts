// Сессия сохраняет canvas и GL-пайплайн между монтированиями компонента темы.
// Это сокращает повторное создание контекстов при переключении тем.
// В CDP-замере от 2026-08-29 каждое такое создание добавляло процессу рендеринга
// 13–46 МБ приватной памяти. Уничтожение контекста, 30 секунд простоя и симуляция
// memory pressure не возвращали эту память в рамках замера.
// acquire() переиспользует сессию, пока isBroken() не обнаружит повреждение.
// После повреждения, invalidate() или release() следующий acquire() создаёт новую.

export type GlSession<TPipeline> = {
  readonly canvas: HTMLCanvasElement;
  readonly pipeline: TPipeline;
};

export class PersistentGlSession<TPipeline> {
  private current: { canvas: HTMLCanvasElement; pipeline: TPipeline } | null = null;

  constructor(
    private readonly createPipeline: (canvas: HTMLCanvasElement) => TPipeline,
    private readonly isBroken: (pipeline: TPipeline) => boolean,
    // Фабрика канваса инъекцией: тесты идут в environment: node, где DOM нет.
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

  /** Полный выпуск сессии: контекст уничтожается (pipeline.destroy()), следующий
   *  acquire() соберёт новый. Дорогая операция — только для скрытого окна
   *  (трей-выгрузка): в живой сессии она вернула бы цикл «создать→убить»,
   *  который и был исходной утечкой. */
  release(): void {
    const s = this.current;
    this.current = null;
    (s?.pipeline as { destroy?: () => void } | undefined)?.destroy?.();
  }

  get alive(): boolean {
    return this.current !== null;
  }
}
