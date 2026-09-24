// Переиспользует CanvasGradient между кадрами, пока не изменились
// геометрия и цвета. Вызывающий код формирует ключ из округлённых
// параметров; после изменения размеров канваса кэш нужно очистить.

export type GradientMemo = {
  (ctx: CanvasRenderingContext2D, key: string, build: () => CanvasGradient): CanvasGradient;
  /** Сброс при изменении геометрии канваса (resize). */
  clear(): void;
};

export function createGradientMemo(): GradientMemo {
  let cache = new Map<string, CanvasGradient>();
  // ctx в сигнатуре — для читаемости вызова; ключ и build замыкают его сами.
  const memo = (
    _ctx: CanvasRenderingContext2D,
    key: string,
    build: () => CanvasGradient,
  ): CanvasGradient => {
    let g = cache.get(key);
    if (g === undefined) {
      g = build();
      cache.set(key, g);
    }
    return g;
  };
  memo.clear = () => {
    cache = new Map();
  };
  return memo;
}

/** Непрерывный параметр → дискретные ступени для ключа кэша. */
export function quant(v: number, step = 0.02): number {
  return Math.round(v / step) * step;
}
