export type BlackCatRig = {
  render(time: number, options?: { strength?: number; neutral?: boolean; exploded?: boolean }): void;
  blink(time: number): void;
  call(time: number): void;
  dispose(): void;
};
export function createBlackCatRig(container: HTMLElement, image: HTMLImageElement): BlackCatRig;
