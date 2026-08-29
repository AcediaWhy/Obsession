// Кэш CanvasGradient для покадровых заливок.
//
// Градиент-объект легально переиспользовать между кадрами (fillStyle/strokeStyle
// принимают его повторно), пока неизменны геометрия и цвета. Раньше Aurora
// строила 3–5 градиентов НА КАДР при каденции монитора — сотни нативных
// аллокаций в секунду, поднимающие high-water аллокатора (а WebView2 отдаёт
// такую память неохотно — см. gl/persistentGlSession).
//
// Ключ собирает вызывающий код из квантованных анимированных параметров:
// цвета после Math.round уже целочисленные, непрерывные величины (warm, beat,
// фликер) — через quant(). Ступени 0.02 на плавном дрейфе глаза не читают.

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
