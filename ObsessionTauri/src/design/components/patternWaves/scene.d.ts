export interface PatternOptions {
  service?: string;
  candidate?: string;
  color?: number[];
  background?: number[];
  intro?: boolean;
  paused?: boolean;
  motionOff?: boolean;
  interactive?: boolean;
  spacing?: number;
  fadeSize?: number;
  maxDpr?: number;
  cursorSize?: number;
  cursorStrength?: number;
  transitionSeconds?: number;
  morph?: number;
  speed?: number;
  opacity?: number;
  onUnavailable?: () => void;
}
export interface PatternScene { update(options: PatternOptions): void; dispose(): void }
export function createPatternWaves(container: HTMLElement, options?: PatternOptions): PatternScene;
