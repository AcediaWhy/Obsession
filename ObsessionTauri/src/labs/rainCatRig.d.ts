import type { ObsessionVisualPhase } from '../design/obsessionVisualState';

export type RainCatPose = {
  breath: number; lean: number; umbrella: number; tail: number;
  blink: number; look: number; earLeft: number; earRight: number;
  stretch: number; squint: number; eyeY: number;
};
export type RainCatOptions = {
  strength?: number; neutral?: boolean; windAt?: number; blinkAtTime?: number;
  state?: ObsessionVisualPhase; stateAt?: number; rain?: boolean; puddle?: boolean; pose?: RainCatPose;
};
export type RainCatImages = Record<'body'|'tail'|'eyes'|'umbrella'|'paw', HTMLImageElement> & { puddle?: HTMLImageElement };
export type RainCatRig = { render(time: number, options?: RainCatOptions): void; dispose(): void };
export function rainCatPose(time: number, options?: RainCatOptions): RainCatPose;
export function createRainCatRig(canvas: HTMLCanvasElement, images: RainCatImages): RainCatRig;
