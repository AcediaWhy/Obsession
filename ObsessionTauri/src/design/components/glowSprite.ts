// Кэш оффскрин-спрайтов свечения для частиц (светлячки, искры, глаза).
//
// Проблема: createRadialGradient на КАЖДУЮ частицу КАЖДЫЙ кадр — десятки-сотни
// аллокаций за кадр (тяжёлые поля доходили до ~129), тысячи в секунду на высокой
// герцовке. Это насыщает main-thread и давит GC — темы лагают, а капнутые
// анимации уходят в слоу-мо по dt-клампу.
//
// Решение: градиент запекается в маленький канвас ОДИН раз на «корзину»
// параметра (обычно warm или rise, квантованные на 8-9 ступеней), в кадре
// остаётся только drawImage. Альфа выносится в ctx.globalAlpha (альфы стопов
// везде линейны по яркости частицы), радиус — в размер drawImage.
//
// Использование:
//   const sprite = createSpriteCache(9, 64, (ctx, px, k) => { ...градиент... });
//   // в кадре:
//   ctx.globalAlpha = a;
//   ctx.drawImage(sprite(warm), x - r, y - r, r * 2, r * 2);
//   // после цикла частиц:
//   ctx.globalAlpha = 1;

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
