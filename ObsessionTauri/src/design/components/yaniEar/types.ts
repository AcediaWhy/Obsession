export type EarMood = "idle" | "busy" | "scanning" | "active" | "alarm";
export type EarSide = -1 | 1;
export type EarQuality = "high" | "balanced" | "low";

// Художественный проход темы. Сейчас есть только "current" — то, что стоит в
// приложении. Новый эксперимент добавляется значением сюда и схемой в
// LIGHT_SCHEMES (YaniCharacterScene): тема при этом не меняется, потому что
// HeroField проп не передаёт и всегда получает "current".
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
