export const RAIN_PARALLAX_SMOOTHING = -Math.log(1 - 0.06) * 60;

export type RainPointerRect = Pick<DOMRect, "left" | "top" | "width" | "height">;

export type RainFramePhases = {
  input(): void;
  smooth(dt: number): void;
  simulate(dt: number): void;
  uploadTexture(): void;
  draw(): void;
};

export function smoothRainValue(
  current: number,
  target: number,
  dt: number,
  smoothing = RAIN_PARALLAX_SMOOTHING,
): number {
  const elapsed = Math.max(0, dt);
  const alpha = 1 - Math.exp(-Math.max(0, smoothing) * elapsed);
  return current + (target - current) * alpha;
}

export function normalizeRainPointer(
  clientX: number,
  clientY: number,
  rect: RainPointerRect,
): { x: number; y: number } {
  const width = Math.max(1, rect.width);
  const height = Math.max(1, rect.height);
  const x = ((clientX - rect.left) / width) * 2 - 1;
  const y = ((clientY - rect.top) / height) * 2 - 1;
  return {
    x: Math.max(-1, Math.min(1, x)),
    y: Math.max(-1, Math.min(1, y)),
  };
}

export function runRainFrame(phases: RainFramePhases, dt: number): void {
  phases.input();
  phases.smooth(dt);
  phases.simulate(dt);
  phases.uploadTexture();
  phases.draw();
}
