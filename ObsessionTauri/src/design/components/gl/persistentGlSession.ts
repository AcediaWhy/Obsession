// Один canvas + один GL-контекст на WebGL-тему, живущий между маунтами поля.
//
// Зачем: каждое переключение темы раньше рождало и хоронило WebGL-контекст
// (конструктор пайплайна + destroy() с loseContext). Замер на установленном
// приложении (CDP-проба, 2026-08-29): каждый визит WebGL-темы добавлял
// renderer-процессу +13..46 МБ приватной памяти, которые не возвращались ни
// после 30 c простоя, ни после симуляции memory pressure, хотя контексты
// терялись корректно (webglcontextlost на каждый destroy) — WebView2 отдаёт
// нативную память контекста неохотно, а в трее стимулов к отдаче нет вовсе.
// Персистентная сессия платит за контекст один раз за жизнь страницы:
// максимум три контекста (yani / choir / rain) вместо N на каждое
// переключение.
//
// Потеря контекста (GPU reset, вытеснение по лимиту Chromium) — не катастрофа:
// пайплайн помечает себя мёртвым через abandonAfterContextLoss(), isBroken
// видит это, и следующий acquire() собирает новый canvas + контекст с нуля.

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
