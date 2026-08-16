import type { QualityTier } from "../../frameScheduler";

export type ObsessionQualityProfile = {
  resolutionScale: number;
  targetFps: 30 | 60;
  threadCount: number;
  caustics: boolean;
  aberration: number;
};

const PROFILES: Record<QualityTier, ObsessionQualityProfile> = {
  high: {
    resolutionScale: 1,
    targetFps: 60,
    threadCount: 16,
    caustics: true,
    aberration: 1,
  },
  balanced: {
    resolutionScale: 0.72,
    targetFps: 30,
    threadCount: 10,
    caustics: true,
    aberration: 0.45,
  },
  low: {
    resolutionScale: 0.52,
    targetFps: 30,
    threadCount: 6,
    caustics: false,
    aberration: 0,
  },
};

export function obsessionQualityProfile(tier: QualityTier): ObsessionQualityProfile {
  return PROFILES[tier];
}
