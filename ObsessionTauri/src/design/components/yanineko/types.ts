import type { ObsessionVisualPhase } from "../../obsessionVisualState";

export type YaniMood = "idle" | "busy" | "scanning" | "active" | "alarm";

export type YaniSignals = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
};
export type YaniMotionInput = {
  time: number;
  mood: YaniMood;
  moodAge: number;
  pointerX?: number;
  pointerY?: number;
  hover?: number;
};

export type YaniMotionFrame = {
  gazeX: number;
  gazeY: number;
  eyeOpen: number;
  pupilScale: number;
  leftEar: number;
  rightEar: number;
  earFlat: number;
  blink: number;
  breath: number;
  hairLift: number;
  cigaretteTremor: number;
  ember: number;
  smoke: number;
  blush: number;
  grimace: number;
  ringGlint: number;
};

export type YaniPanelLens = {
  x: number;
  y: number;
  width: number;
  height: number;
  radius: number;
};

export type YaniFieldFrame = {
  time: number;
  dt: number;
  phase: ObsessionVisualPhase;
  pointerX: number;
  pointerY: number;
  pointerVx: number;
  pointerVy: number;
  sceneShiftX: number;
  sceneShiftY: number;
  panels: readonly YaniPanelLens[];
};
