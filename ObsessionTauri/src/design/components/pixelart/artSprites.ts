import { type BakedArt, TRANSPARENT } from "./artBake";

// Слои, которые дал автор, оказались не плоским композитом, а ЛИСТАМИ АССЕТОВ: стена с
// полкой отдельно, семь горшков и растений в сетке, восемь прядей плюща, кот отдельно.
// Значит композицию надо собрать заново — и собрать её ровно так, как у автора.
//
// Позиции не угадываются: каждый спрайт ищется шаблонным сопоставлением по готовой
// композиции `lets-go.png`. Так восстанавливается АВТОРСКАЯ раскладка, а не моя догадка.
// Поиск двухступенчатый: грубый по сетке через четыре пикселя, затем уточнение ±4.

export type Sprite = {
  /** Имя листа, с которого снят спрайт. */
  sheet: string;
  /** Номер по убыванию площади внутри листа — стабильный идентификатор. */
  pick: number;
  width: number;
  height: number;
  /** Индексы палитры по строкам, TRANSPARENT вне силуэта. */
  indices: Uint8Array;
  /** Сколько непрозрачных пикселей — для порогов и для оценки совпадения. */
  solid: number;
};

export type Placement = {
  sheet: string;
  pick: number;
  x: number;
  y: number;
  /** Средняя ошибка канала на непрозрачный пиксель: чем меньше, тем надёжнее позиция. */
  error: number;
};

/** Связные компоненты непрозрачных пикселей: каждый остров — отдельный предмет. */
export function extractSprites(art: BakedArt, sheet: string, minPixels = 60): Sprite[] {
  const layer = art.layers.find((item) => item.name === sheet);
  if (!layer) return [];
  const size = art.size;
  const seen = new Uint8Array(size * size);
  const found: { x: number; y: number; w: number; h: number; cells: number[] }[] = [];

  for (let start = 0; start < size * size; start += 1) {
    if (seen[start] || layer.indices[start] === TRANSPARENT) continue;
    const stack = [start];
    seen[start] = 1;
    const cells: number[] = [];
    let minX = size;
    let minY = size;
    let maxX = -1;
    let maxY = -1;
    while (stack.length > 0) {
      const index = stack.pop() as number;
      const x = index % size;
      const y = (index - x) / size;
      cells.push(index);
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      // Восемь соседей: тонкие стебли плюща по диагонали иначе рвутся на куски.
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [-1, -1], [1, -1], [-1, 1]]) {
        const nx = x + dx;
        const ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= size || ny >= size) continue;
        const next = ny * size + nx;
        if (seen[next] || layer.indices[next] === TRANSPARENT) continue;
        seen[next] = 1;
        stack.push(next);
      }
    }
    if (cells.length >= minPixels) found.push({ x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1, cells });
  }

  found.sort((first, second) => second.cells.length - first.cells.length);
  return found.map((island, pick) => {
    const indices = new Uint8Array(island.w * island.h).fill(TRANSPARENT);
    for (const cell of island.cells) {
      const x = cell % size;
      const y = (cell - x) / size;
      indices[(y - island.y) * island.w + (x - island.x)] = layer.indices[cell];
    }
    return { sheet, pick, width: island.w, height: island.h, indices, solid: island.cells.length };
  });
}

function channels(hex: string) {
  return [
    Number.parseInt(hex.slice(1, 3), 16),
    Number.parseInt(hex.slice(3, 5), 16),
    Number.parseInt(hex.slice(5, 7), 16),
  ] as const;
}

/**
 * Сопоставление по цвету, а не по индексу: один и тот же блок в листе и в композиции мог
 * округлиться к разным записям палитры, потому что у него разные соседи и разный шум.
 */
function scoreAt(
  target: Uint8Array,
  size: number,
  palette: readonly (readonly [number, number, number])[],
  sprite: Sprite,
  offsetX: number,
  offsetY: number,
  step: number,
) {
  let sum = 0;
  let count = 0;
  for (let y = 0; y < sprite.height; y += step) {
    const ty = offsetY + y;
    if (ty < 0 || ty >= size) return Number.POSITIVE_INFINITY;
    for (let x = 0; x < sprite.width; x += step) {
      const value = sprite.indices[y * sprite.width + x];
      if (value === TRANSPARENT) continue;
      const tx = offsetX + x;
      if (tx < 0 || tx >= size) return Number.POSITIVE_INFINITY;
      const other = target[ty * size + tx];
      if (other === TRANSPARENT) return Number.POSITIVE_INFINITY;
      const a = palette[value];
      const b = palette[other];
      sum += Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]);
      count += 3;
    }
  }
  if (count === 0) return Number.POSITIVE_INFINITY;
  return sum / count;
}

/** Двухступенчатый поиск: грубая сетка через `coarse`, затем уточнение в окне ±coarse. */
export function matchSprite(
  target: Uint8Array,
  size: number,
  paletteHex: readonly string[],
  sprite: Sprite,
  coarse = 4,
): Placement {
  const palette = paletteHex.map(channels);
  let best = { x: 0, y: 0, error: Number.POSITIVE_INFINITY };
  for (let y = 0; y <= size - sprite.height; y += coarse) {
    for (let x = 0; x <= size - sprite.width; x += coarse) {
      const error = scoreAt(target, size, palette, sprite, x, y, coarse);
      if (error < best.error) best = { x, y, error };
    }
  }
  const from = { x: best.x, y: best.y };
  for (let y = from.y - coarse; y <= from.y + coarse; y += 1) {
    for (let x = from.x - coarse; x <= from.x + coarse; x += 1) {
      if (x < 0 || y < 0 || x > size - sprite.width || y > size - sprite.height) continue;
      const error = scoreAt(target, size, palette, sprite, x, y, 1);
      if (error < best.error) best = { x, y, error };
    }
  }
  return { sheet: sprite.sheet, pick: sprite.pick, x: best.x, y: best.y, error: +best.error.toFixed(2) };
}
