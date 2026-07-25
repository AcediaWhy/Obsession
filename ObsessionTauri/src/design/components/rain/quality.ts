import type { QualityTier } from "../../render";

export type RainQualityProfile = {
  /** Масштаб backing-разрешения сцены (и водной карты). */
  waterScale: number;
  /** Масштаб FBO мира относительно backing (мир сам по себе мягкий). */
  worldScale: number;
  /** Глубина мип-цепочки мира, доступная фокусу композита. */
  mipDepth: number;
  /** Кап капель codrops-симуляции (внутри домножается на areaMultiplier). */
  maxDrops: number;
  /** Разрешение сетки конденсата по ширине окна. */
  mistGrid: number;
};

const PROFILES: Record<QualityTier, RainQualityProfile> = {
  high: { waterScale: 1, worldScale: 1, mipDepth: 6, maxDrops: 900, mistGrid: 160 },
  balanced: { waterScale: 0.75, worldScale: 0.75, mipDepth: 5, maxDrops: 600, mistGrid: 112 },
  low: { waterScale: 0.5, worldScale: 0.5, mipDepth: 4, maxDrops: 350, mistGrid: 80 },
};

export function rainQualityProfile(tier: QualityTier): RainQualityProfile {
  return PROFILES[tier];
}
