// Восстанавливает пиксельные слои из изображений с артефактами JPEG.
// По измеренным границам блоков исходная сетка — 256×256, увеличенная вчетверо.
// Средний цвет блока 4×4 подавляет шум сжатия; все слои затем приводятся
// к общей палитре для согласованной анимации цвета.

/** Замеренный размер блока: 1024 / 4 = 256 арт-пикселей. */
export const ART_BLOCK = 4;
export const ART_SIZE = 256;

/** Индекс прозрачности. Палитра поэтому не длиннее 255 цветов. */
export const TRANSPARENT = 255;

export type BakedLayer = {
  name: string;
  /** Индексы палитры по строкам, TRANSPARENT там, где слой прозрачен. */
  indices: Uint8Array;
};

export type BakedArt = {
  size: number;
  palette: string[];
  layers: BakedLayer[];
  /** Сколько цветов реально понадобилось — для отчёта и тестов. */
  paletteSize: number;
};

export type ArtSource = {
  name: string;
  url: string;
  /**
   * Как вырезать фон.
   *   none  — слой непрозрачный целиком (фон композиции);
   *   flood — заливкой ОТ РАМКИ: защищает светлые области внутри силуэта. Нужен коту,
   *           у него белки глаз почти белые;
   *   light — все светлые нейтральные пиксели, включая замкнутые карманы между листьями.
   *           Нужен плющу и горшкам: заливка от рамки оставляла в листве белые дырки,
   *           потому что до этих карманов ей не дойти.
   */
  key: "none" | "flood" | "light";
};

type Rgb = { r: number; g: number; b: number };

async function decode(url: string) {
  const image = new Image();
  image.decoding = "sync";
  image.src = url;
  await image.decode();
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("2d context unavailable");
  context.imageSmoothingEnabled = false;
  context.drawImage(image, 0, 0);
  return context.getImageData(0, 0, canvas.width, canvas.height);
}

/** Среднее по блоку: именно здесь и умирает шум JPEG. */
function downsample(source: ImageData, size: number, block: number) {
  const out = new Float32Array(size * size * 3);
  for (let ay = 0; ay < size; ay += 1) {
    for (let ax = 0; ax < size; ax += 1) {
      let r = 0;
      let g = 0;
      let b = 0;
      let count = 0;
      for (let dy = 0; dy < block; dy += 1) {
        const y = ay * block + dy;
        if (y >= source.height) continue;
        for (let dx = 0; dx < block; dx += 1) {
          const x = ax * block + dx;
          if (x >= source.width) continue;
          const index = (y * source.width + x) * 4;
          r += source.data[index];
          g += source.data[index + 1];
          b += source.data[index + 2];
          count += 1;
        }
      }
      const at = (ay * size + ax) * 3;
      out[at] = r / Math.max(1, count);
      out[at + 1] = g / Math.max(1, count);
      out[at + 2] = b / Math.max(1, count);
    }
  }
  return out;
}

/**
 * Прозрачность вырезается ЗАЛИВКОЙ ОТ РАМКИ, а не по цвету. Разница принципиальная: у кота
 * белки глаз почти того же цвета, что фон слоя, и глобальный ключ по цвету выбил бы их.
 * Заливка же доходит только до связного фона.
 *
 * Критерий — «светлый и нейтральный по каналам», и заливка идёт только от рамки. Порог
 * широкий нарочно: слой кота экспортирован вместе с ШАШКОЙ прозрачности редактора, то есть
 * фон там честно чередует #dcdcdc и #ffffff блоками по четыре пикселя. Узкий порог на
 * близость к соседу спотыкался на каждой клетке шашки и оставлял слой непрозрачным.
 * Белки глаз кота при этом выживают: они замкнуты тёмным контуром, и заливка до них не
 * доходит.
 */
function keyBackgroundMask(pixels: Float32Array, size: number, mode: "flood" | "light") {
  const mask = new Uint8Array(size * size);
  const lightNeutral = (index: number) => {
    const r = pixels[index * 3];
    const g = pixels[index * 3 + 1];
    const b = pixels[index * 3 + 2];
    const min = Math.min(r, g, b);
    return min > 200 && Math.max(r, g, b) - min < 30;
  };

  if (mode === "light") {
    for (let index = 0; index < size * size; index += 1) if (lightNeutral(index)) mask[index] = 1;
    return mask;
  }

  const queue: number[] = [];
  const push = (index: number) => {
    if (mask[index] || !lightNeutral(index)) return;
    mask[index] = 1;
    queue.push(index);
  };
  for (let x = 0; x < size; x += 1) {
    push(x);
    push((size - 1) * size + x);
  }
  for (let y = 0; y < size; y += 1) {
    push(y * size);
    push(y * size + size - 1);
  }
  while (queue.length > 0) {
    const index = queue.pop() as number;
    const x = index % size;
    const y = (index - x) / size;
    if (x > 0) push(index - 1);
    if (x < size - 1) push(index + 1);
    if (y > 0) push(index - size);
    if (y < size - 1) push(index + size);
  }
  return mask;
}

function hex(value: Rgb) {
  const part = (channel: number) => Math.max(0, Math.min(255, Math.round(channel))).toString(16).padStart(2, "0");
  return `#${part(value.r)}${part(value.g)}${part(value.b)}`;
}

/**
 * Палитра по частоте с отсевом близких. Медианный разрез был бы точнее в общем случае, но у
 * плоского пиксель-арта цвета и без него стоят островами: после усреднения блоков хватает
 * взять самые частые и выкинуть тех, кто ближе порога к уже принятым.
 */
function buildPalette(samples: Float32Array[], masks: (Uint8Array | null)[], size: number, limit: number) {
  const histogram = new Map<number, { count: number; r: number; g: number; b: number }>();
  for (const [layer, pixels] of samples.entries()) {
    const mask = masks[layer];
    for (let index = 0; index < size * size; index += 1) {
      if (mask && mask[index]) continue;
      const r = pixels[index * 3];
      const g = pixels[index * 3 + 1];
      const b = pixels[index * 3 + 2];
      // Ключ по 5 битам на канал: шум JPEG после усреднения ещё гуляет на единицы.
      const key = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
      const bucket = histogram.get(key);
      if (bucket) {
        bucket.count += 1;
        bucket.r += r;
        bucket.g += g;
        bucket.b += b;
      } else histogram.set(key, { count: 1, r, g, b });
    }
  }
  const ranked = [...histogram.values()]
    .map((bucket) => ({ count: bucket.count, r: bucket.r / bucket.count, g: bucket.g / bucket.count, b: bucket.b / bucket.count }))
    .sort((first, second) => second.count - first.count);

  const chosen: Rgb[] = [];
  for (const candidate of ranked) {
    if (chosen.length >= limit) break;
    let tooClose = false;
    for (const accepted of chosen) {
      const distance = (accepted.r - candidate.r) ** 2 + (accepted.g - candidate.g) ** 2 + (accepted.b - candidate.b) ** 2;
      if (distance < 220) {
        tooClose = true;
        break;
      }
    }
    if (!tooClose) chosen.push({ r: candidate.r, g: candidate.g, b: candidate.b });
  }
  return chosen;
}

function nearest(palette: Rgb[], r: number, g: number, b: number) {
  let best = 0;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const [index, colour] of palette.entries()) {
    const distance = (colour.r - r) ** 2 + (colour.g - g) ** 2 + (colour.b - b) ** 2;
    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  }
  return best;
}

export async function bakeArt(sources: readonly ArtSource[], limit = 48): Promise<BakedArt> {
  const size = ART_SIZE;
  const sampled: Float32Array[] = [];
  const masks: (Uint8Array | null)[] = [];

  for (const source of sources) {
    const image = await decode(source.url);
    const pixels = downsample(image, size, ART_BLOCK);
    sampled.push(pixels);
    masks.push(source.key === "none" ? null : keyBackgroundMask(pixels, size, source.key));
  }

  const palette = buildPalette(sampled, masks, size, limit);
  const layers: BakedLayer[] = sources.map((source, layer) => {
    const pixels = sampled[layer];
    const mask = masks[layer];
    const indices = new Uint8Array(size * size);
    for (let index = 0; index < size * size; index += 1) {
      if (mask && mask[index]) {
        indices[index] = TRANSPARENT;
        continue;
      }
      indices[index] = nearest(palette, pixels[index * 3], pixels[index * 3 + 1], pixels[index * 3 + 2]);
    }
    return { name: source.name, indices };
  });

  return { size, palette: palette.map(hex), layers, paletteSize: palette.length };
}

/** Слои композиции. Порядок = порядок наложения; базовый слой идёт первым и без альфы. */
export const SHOP_SOURCES: readonly ArtSource[] = [
  { name: "wall", url: "/%D1%81%D1%82%D0%B5%D0%BD%D0%B0.png", key: "none" },
  { name: "pots", url: "/%D0%B3%D0%BE%D1%80%D1%88%D0%BA%D0%B8.png", key: "light" },
  { name: "cat", url: "/%D0%BA%D0%BE%D1%82%D0%B8%D0%BA.png", key: "flood" },
  { name: "ivy", url: "/%D0%BF%D0%BB%D1%8E%D1%89.png", key: "light" },
];
