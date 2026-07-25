export type HybridRainFramePhases = {
  input(): void;
  weather(dt: number): void;
  simulate(dt: number): void;
  uploadWaterMap(): void;
  worldPass(): void;
  compositePass(): void;
};

export function runHybridRainFrame(phases: HybridRainFramePhases, dt: number): void {
  phases.input();
  phases.weather(dt);
  phases.simulate(dt);
  phases.uploadWaterMap();
  phases.worldPass();
  phases.compositePass();
}
