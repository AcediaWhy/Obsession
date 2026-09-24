// Кэширует спрайты свечения для разных значений параметра. Градиент
// строится при первом обращении к каждой ступени; в кадре вызывающий код
// меняет прозрачность через globalAlpha и размер через drawImage.

/**
 * Ленивый кэш из `buckets + 1` спрайтов px×px по параметру 0..1.
 * `render` вызывается один раз на корзину с k = i/buckets.
 */
export function createSpriteCache(
  buckets: number,
  px: number,
  render: (ctx: CanvasRenderingContext2D, px: number, k: number) => void,
): (k01: number) => HTMLCanvasElement {
  const cache: (HTMLCanvasElement | null)[] = new Array(buckets + 1).fill(null);
  return (k01: number) => {
    const i = Math.max(0, Math.min(buckets, Math.round(k01 * buckets)));
    let c = cache[i];
    if (!c) {
      c = document.createElement("canvas");
      c.width = c.height = px;
      render(c.getContext("2d")!, px, i / buckets);
      cache[i] = c;
    }
    return c;
  };
}
