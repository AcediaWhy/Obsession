// Конденсат на стекле: грубая CPU-сетка уровня запотевания. Растёт к
// потолку (сильнее в дождь — стекло холоднее), протирается ползущими
// каплями и медленно зарастает обратно. Рендерится в маленький canvas,
// который пайплайн заливает в текстуру (композит размывает мир по ней).

export type MistWipe = {
  /** Нормированный центр протирки, 0..1 по обеим осям. */
  x: number;
  y: number;
  /** Нормированный радиус (доля ширины). */
  radius: number;
};

const GROWTH_RATE = 1 / 26; // с⁻¹ — конденсат набирается за ~полминуты
const REGROW_DELAY_POWER = 1.6; // свежепротёртое зарастает медленнее (плёнка воды)

export class RainMistSim {
  private cols: number;
  private rows: number;
  private level: Float32Array;

  constructor(cols: number, rows: number, initial = 0.42) {
    this.cols = Math.max(2, Math.round(cols));
    this.rows = Math.max(2, Math.round(rows));
    this.level = new Float32Array(this.cols * this.rows).fill(initial);
  }

  get size(): { cols: number; rows: number } {
    return { cols: this.cols, rows: this.rows };
  }

  /** Смена сетки: уровень сбрасывается на средний (ресайз — редкое событие). */
  resize(cols: number, rows: number): void {
    const nextCols = Math.max(2, Math.round(cols));
    const nextRows = Math.max(2, Math.round(rows));
    if (nextCols === this.cols && nextRows === this.rows) return;
    const mean = this.mean();
    this.cols = nextCols;
    this.rows = nextRows;
    this.level = new Float32Array(nextCols * nextRows).fill(mean);
  }

  /**
   * Рост конденсата к потолку и протирание каплями. dt в секундах;
   * activity 0..1 — интенсивность дождя (холоднее стекло → выше потолок).
   */
  step(dt: number, activity: number, wipes: readonly MistWipe[]): void {
    const clamped = Math.max(0, Math.min(dt, 0.1));
    const cap = 0.5 + 0.38 * Math.max(0, Math.min(1, activity));
    // Экспоненциальное насыщение, стабильное при любом fps.
    const growth = 1 - Math.exp(-clamped * GROWTH_RATE);
    const level = this.level;
    for (let i = 0; i < level.length; i += 1) {
      const current = level[i];
      const toward = cap - current;
      if (toward > 0) {
        // Полностью протёртые зоны зарастают медленнее: сначала стекает вода.
        level[i] = current + toward * growth * Math.pow(current / cap + 0.25, REGROW_DELAY_POWER - 1);
      } else {
        level[i] = current + toward * growth * 2;
      }
    }
    for (const wipe of wipes) this.applyWipe(wipe);
  }

  /** Средний уровень — для тестов и ресайза. */
  mean(): number {
    let sum = 0;
    for (let i = 0; i < this.level.length; i += 1) sum += this.level[i];
    return this.level.length > 0 ? sum / this.level.length : 0;
  }

  levelAt(x: number, y: number): number {
    const cx = Math.max(0, Math.min(this.cols - 1, Math.round(x * (this.cols - 1))));
    const cy = Math.max(0, Math.min(this.rows - 1, Math.round(y * (this.rows - 1))));
    return this.level[cy * this.cols + cx];
  }

  /** Заливает уровень в RGBA ImageData (R=уровень, A=255). */
  writeTo(data: Uint8ClampedArray): void {
    const level = this.level;
    for (let i = 0; i < level.length; i += 1) {
      const value = Math.max(0, Math.min(255, Math.round(level[i] * 255)));
      const offset = i * 4;
      data[offset] = value;
      data[offset + 1] = value;
      data[offset + 2] = value;
      data[offset + 3] = 255;
    }
  }

  private applyWipe(wipe: MistWipe): void {
    const cx = wipe.x * (this.cols - 1);
    const cy = wipe.y * (this.rows - 1);
    const radius = Math.max(1, wipe.radius * this.cols);
    const minX = Math.max(0, Math.floor(cx - radius));
    const maxX = Math.min(this.cols - 1, Math.ceil(cx + radius));
    const minY = Math.max(0, Math.floor(cy - radius));
    const maxY = Math.min(this.rows - 1, Math.ceil(cy + radius));
    const invRadius = 1 / radius;
    for (let y = minY; y <= maxY; y += 1) {
      for (let x = minX; x <= maxX; x += 1) {
        const dx = (x - cx) * invRadius;
        const dy = (y - cy) * invRadius;
        const dist = dx * dx + dy * dy;
        if (dist >= 1) continue;
        // Мягкий край протирки; в центре стирает почти в ноль.
        const strength = 1 - dist;
        const index = y * this.cols + x;
        this.level[index] *= Math.max(0, 1 - strength * 0.9);
      }
    }
  }
}
