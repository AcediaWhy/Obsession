export interface AlchemistTwitch { tail: number; leftEar: number; rightEar: number }

export function alchemistTwitchAt(time: number): AlchemistTwitch {
  const t = ((time % 14000) + 14000) % 14000;
  const pulse = (start: number, step: number, values: readonly number[]) => {
    const index = Math.floor((t - start) / step);
    return index >= 0 && index < values.length ? values[index] : 0;
  };
  return {
    tail: pulse(1800, 120, [8, 16, 24, 16, 0, -8, 0]) + pulse(7900, 140, [-8, -16, -24, -16, -8, 0]),
    leftEar: pulse(3400, 85, [-8, -16, -8, 0, 8, 0]),
    rightEar: pulse(3600, 85, [8, 16, 8, 0]),
  };
}

// Source-stage rectangles include empty space above each tip. The bottom is
// anchored: displacement tapers to zero, keeping the join to the body fixed.
export const twitchRegions = {
  tail: [184, 778, 190, 250],
  leftEar: [332, 236, 105, 94],
  rightEar: [849, 274, 114, 75],
} as const;

export function twitchRowShift(amplitude: number, row: number, height: number, scale: number): number {
  const influence = Math.max(0, 1 - row / Math.max(1, height - 1));
  return Math.round(amplitude * scale * influence * influence) || 0;
}
