export type EarMood = "idle" | "busy" | "scanning" | "active" | "alarm";
export type EarSide = -1 | 1;
export type EarQuality = "high" | "balanced" | "low";

// Вариант освещения персонажа. Приложение использует только "current";
// схема для него задана в LIGHT_SCHEMES (YaniCharacterScene).
export type YaniArtPass = "current";

export type EarTargetInput = {
  time: number;
  mood: EarMood;
  side: EarSide;
  pointerX: number;
  pointerY: number;
  pointerActive: number;
};

export type EarPose = {
  yaw: number;
  pitch: number;
  splay: number;
  cup: number;
  tip: number;
  lift: number;
  ringSwing: number;
};

export type EarSpring = {
  value: number;
  velocity: number;
};

export type EarSpringPose = Record<keyof EarPose, EarSpring>;

export type EarQualityProfile = {
  pixelRatio: number;
  shellCount: number;
  hairRatio: number;
  textureSize: number;
  shadowSize: number;
};
