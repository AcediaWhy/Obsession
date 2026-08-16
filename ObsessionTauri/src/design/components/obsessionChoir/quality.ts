import type { QualityTier } from "../../frameScheduler";

export type ObsessionChoirQuality = {
  resolutionScale: number;
  targetFps: 30 | 60;
  curveCount: 24 | 36 | 48;
  curveSegments: 64 | 96 | 144;
  panelCount: 6 | 9 | 12;
  caustics: boolean;
  aberration: number;
  refraction: number;
};

const QUALITY: Record<QualityTier, ObsessionChoirQuality> = {
  high: {
    resolutionScale: 1.2,
    targetFps: 60,
    curveCount: 48,
    curveSegments: 144,
    panelCount: 12,
    caustics: true,
    aberration: 1,
    refraction: 1,
  },
  balanced: {
    resolutionScale: 1,
    targetFps: 30,
    curveCount: 36,
    curveSegments: 96,
    panelCount: 9,
    caustics: true,
    aberration: 0.42,
    refraction: 0.72,
  },
  low: {
    resolutionScale: 0.82,
    targetFps: 30,
    curveCount: 24,
    curveSegments: 64,
    panelCount: 6,
    caustics: false,
    aberration: 0,
    refraction: 0.42,
  },
};

export function obsessionChoirQuality(tier: QualityTier): ObsessionChoirQuality {
  return QUALITY[tier];
}
