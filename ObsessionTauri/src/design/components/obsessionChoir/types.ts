import type { ObsessionVisualPhase } from "../../obsessionVisualState";

export type ObsessionPanelLens = {
  x: number;
  y: number;
  width: number;
  height: number;
  radius: number;
};

export type ObsessionChoirMotionSample = {
  masterGazeX: number;
  masterGazeY: number;
  masterLidOpen: number;
  masterBodyX: number;
  masterBodyY: number;
  masterRoll: number;
  masterPulse: number;
  pupilScale: number;
  chorusReveal: number;
  chorusAlignment: number;
  lineTension: number;
  lineFlow: number;
  apertureOpen: number;
  carmineDepth: number;
  faultShear: number;
  ritual: number;
};

export type ObsessionChoirMotionInput = {
  time: number;
  phase: ObsessionVisualPhase;
  phaseAge: number;
  forceRitual?: boolean;
};

export type ObsessionChoirFrame = ObsessionChoirMotionSample & {
  time: number;
  phase: ObsessionVisualPhase;
  focusX: number;
  focusY: number;
  pointerX: number;
  pointerY: number;
  panels: readonly ObsessionPanelLens[];
};
