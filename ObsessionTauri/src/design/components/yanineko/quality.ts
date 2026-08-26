import type { QualityTier } from "../../frameScheduler";

export type YaniQualityProfile = {
  maxFps: number;
  worldScale: number;
  smokeScale: number;
  noiseOctaves: number;
  panelCount: number;
  dustCount: number;
};

const PROFILES: Record<QualityTier, YaniQualityProfile> = {
  high: { maxFps: 180, worldScale: 0.9, smokeScale: 0.5, noiseOctaves: 5, panelCount: 12, dustCount: 64 },
  balanced: { maxFps: 120, worldScale: 0.75, smokeScale: 0.38, noiseOctaves: 4, panelCount: 8, dustCount: 40 },
  low: { maxFps: 60, worldScale: 0.6, smokeScale: 0.28, noiseOctaves: 3, panelCount: 6, dustCount: 24 },
};

export function yaniQuality(tier: QualityTier): YaniQualityProfile {
  return PROFILES[tier];
}
