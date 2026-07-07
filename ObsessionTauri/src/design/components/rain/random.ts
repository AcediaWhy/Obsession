// Порт random.js.
export function random(
  from: number | null = null,
  to: number | null = null,
  interpolation: ((n: number) => number) | null = null,
): number {
  if (from == null) {
    from = 0;
    to = 1;
  } else if (from != null && to == null) {
    to = from;
    from = 0;
  }
  const delta = (to as number) - from;
  if (interpolation == null) interpolation = (n) => n;
  return from + interpolation(Math.random()) * delta;
}

export function chance(c: number): boolean {
  return random() <= c;
}

export function times(n: number, f: (i: number) => void): void {
  for (let i = 0; i < n; i++) f(i);
}

export function createCanvas(width: number, height: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}
