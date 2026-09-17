export type GoldenMeadowOptions = {
  wind?: number;
  windTime?: number;
  earTime?: number;
  blinkTime?: number;
  tailBoost?: number;
  petTime?: number;
  attentionTime?: number;
  attentionSide?: number;
  gustTime?: number;
  gustStrength?: number;
  gustDirection?: number;
};

export function createGoldenMeadow(canvas: HTMLCanvasElement | OffscreenCanvas, configuration?: { scale?: number; cacheBackground?: boolean }): {
  render(time: number, options?: GoldenMeadowOptions): void;
  dispose(): void;
  dimensions: { width: number; height: number };
  diagnostics: Record<string, string>;
};
