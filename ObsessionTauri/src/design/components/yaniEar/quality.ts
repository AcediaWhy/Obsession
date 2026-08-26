import type { EarQuality, EarQualityProfile } from "./types";

const EAR_QUALITY: Record<EarQuality, EarQualityProfile> = {
  high: { pixelRatio: 2, shellCount: 8, hairRatio: 1, textureSize: 512, shadowSize: 2048 },
  balanced: { pixelRatio: 1.5, shellCount: 5, hairRatio: 0.58, textureSize: 384, shadowSize: 1024 },
  low: { pixelRatio: 1, shellCount: 3, hairRatio: 0.28, textureSize: 256, shadowSize: 512 },
};

export function earQualityProfile(quality: EarQuality): EarQualityProfile {
  return EAR_QUALITY[quality];
}
