export type MeadowReaction = {
  attentionTime: number;
  attentionSide: number;
  attentionCount: number;
  gustTime: number;
  gustStrength: number;
  gustDirection: number;
  gustCount: number;
};

export function createMeadowInteraction(): {
  reset(): void;
  move(x: number, y: number, time: number, pointerId?: number): void;
  sample(time: number): MeadowReaction;
};
