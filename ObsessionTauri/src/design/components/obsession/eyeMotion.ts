import type { ObsessionVisualPhase } from "../../obsessionVisualState";

export type ObsessionEyeMotionSample = {
  gazeX: number;
  gazeY: number;
  lidOpen: number;
  pupilScale: number;
  bodyTension: number;
  irisRotation: number;
  highlightPhase: number;
  fixation: number;
  faultSplit: number;
};

export type ObsessionEyeMotionInput = {
  time: number;
  phase: ObsessionVisualPhase;
  phaseAge: number;
  pointerX?: number;
  pointerY?: number;
};

const TAU = Math.PI * 2;

function clamp(value: number, low = 0, high = 1): number {
  return Math.max(low, Math.min(high, value));
}

function fract(value: number): number {
  return value - Math.floor(value);
}

function smoothstep(low: number, high: number, value: number): number {
  const x = clamp((value - low) / Math.max(0.00001, high - low));
  return x * x * (3 - 2 * x);
}

function hash(index: number, salt: number): number {
  return fract(Math.sin(index * 127.1 + salt * 311.7) * 43758.5453123);
}

function pulse(value: number, start: number, peak: number, end: number): number {
  return smoothstep(start, peak, value) * (1 - smoothstep(peak, end, value));
}

function saccadeTarget(index: number, phase: ObsessionVisualPhase): { x: number; y: number } {
  const spread = phase === "scanning" ? 1 : phase === "fault" ? 0.82 : 0.68;
  return {
    x: (hash(index, 1.71) * 2 - 1) * spread,
    y: (hash(index, 8.43) * 2 - 1) * spread * 0.56,
  };
}

function autonomousGaze(time: number, phase: ObsessionVisualPhase): { x: number; y: number } {
  const scanning = phase === "scanning";
  const baseInterval = scanning ? 0.49 : 1.78;
  const warp = time / baseInterval + Math.sin(time * (scanning ? 1.13 : 0.43)) * (scanning ? 0.15 : 0.22);
  const index = Math.floor(warp);
  const local = fract(warp);
  const from = saccadeTarget(index - 1, phase);
  const to = saccadeTarget(index, phase);
  const travel = smoothstep(0.02, scanning ? 0.34 : 0.16, local);
  return {
    x: from.x + (to.x - from.x) * travel,
    y: from.y + (to.y - from.y) * travel,
  };
}

function idleFixation(time: number): number {
  // The warped 15 second clock produces roughly 12–18 second intervals while
  // remaining fully deterministic for every renderer.
  const warped = time / 15 + Math.sin(time * 0.18 + 0.7) * 0.055;
  const local = fract(warped);
  const cycle = Math.floor(warped);
  const start = 0.69 + hash(cycle, 4.2) * 0.055;
  const duration = 0.08 + hash(cycle, 6.6) * 0.04;
  return pulse(local, start, start + 0.025, start + duration);
}

function idleBlink(time: number): number {
  const warped = time / 5.35 + Math.sin(time * 0.31 + 1.4) * 0.095;
  const local = fract(warped);
  const cycle = Math.floor(warped);
  const primary = pulse(local, 0.72, 0.775, 0.835);
  const doubleBlink = hash(cycle, 12.7) > 0.68
    ? pulse(local, 0.87, 0.91, 0.965) * 0.82
    : 0;
  return clamp(Math.max(primary, doubleBlink));
}

export function sampleObsessionEyeMotion({
  time,
  phase,
  phaseAge,
  pointerX = 0,
  pointerY = 0,
}: ObsessionEyeMotionInput): ObsessionEyeMotionSample {
  const safeTime = Number.isFinite(time) ? Math.max(0, time) : 0;
  const safeAge = Number.isFinite(phaseAge) ? Math.max(0, phaseAge) : 0;
  const autonomous = autonomousGaze(safeTime, phase);
  const fixation = phase === "idle" ? idleFixation(safeTime) : phase === "focused" ? 0.82 : 0;
  const attention = phase === "idle"
    ? Math.pow(Math.max(0, Math.sin(safeTime * 0.37 - 0.8)), 10) * 0.12 * (1 - fixation)
    : 0;

  let gazeX = autonomous.x * (1 - fixation);
  let gazeY = autonomous.y * (1 - fixation);
  gazeX += clamp(pointerX, -1, 1) * attention;
  gazeY += clamp(pointerY, -1, 1) * attention * 0.55;

  const idleBodyBreath = 0.5 + 0.5 * Math.sin(safeTime * 0.82 + Math.sin(safeTime * 0.23) * 0.65);
  let lidOpen = (0.8 + idleBodyBreath * 0.19) * (1 - idleBlink(safeTime) * 0.94);
  let pupilScale = 0.9 + Math.sin(safeTime * 0.73) * 0.07 + fixation * 0.32;
  let bodyTension = 0.24 + idleBodyBreath * 0.42;
  let faultSplit = 0;

  if (phase === "engaging") {
    const close = smoothstep(0.03, 0.2, safeAge);
    const reopen = smoothstep(0.28, 1.18, safeAge);
    lidOpen = 1 - close * 0.68 + reopen * 0.68;
    pupilScale = 0.67 + reopen * 0.18;
    bodyTension = 0.82 - reopen * 0.18;
    gazeX *= 0.3;
    gazeY *= 0.3;
  } else if (phase === "scanning") {
    lidOpen = 0.73 + Math.sin(safeTime * 2.7) * 0.09;
    pupilScale = 0.7 + Math.sin(safeTime * 3.9) * 0.11;
    bodyTension = 0.88;
  } else if (phase === "focused") {
    gazeX = Math.sin(safeTime * 0.39) * 0.045;
    gazeY = Math.cos(safeTime * 0.31) * 0.025;
    lidOpen = 0.94 + Math.sin(safeTime * 0.41) * 0.025;
    pupilScale = 1.06 + Math.sin(safeTime * 0.37) * 0.025;
    bodyTension = 0.42;
  } else if (phase === "fault") {
    const jerk = Math.sin(safeTime * 9.1) * Math.sin(safeTime * 3.7 + 0.8);
    gazeX = clamp(autonomous.x * 0.76 + jerk * 0.28, -1, 1);
    gazeY = clamp(autonomous.y * 0.72 + Math.cos(safeTime * 8.3) * 0.18, -1, 1);
    lidOpen = clamp(0.69 + Math.sin(safeTime * 5.4) * 0.22, 0.34, 1);
    pupilScale = clamp(0.92 + jerk * 0.25, 0.62, 1.2);
    bodyTension = 1;
    faultSplit = 0.45 + Math.abs(Math.sin(safeTime * 4.8)) * 0.55;
  }

  return {
    gazeX: clamp(gazeX, -1, 1),
    gazeY: clamp(gazeY, -1, 1),
    lidOpen: clamp(lidOpen, 0.04, 1),
    pupilScale: clamp(pupilScale, 0.55, 1.35),
    bodyTension: clamp(bodyTension),
    irisRotation: fract(safeTime * (phase === "scanning" ? 0.19 : phase === "fault" ? -0.12 : 0.052)) * TAU,
    highlightPhase: fract(safeTime * (phase === "scanning" ? 0.31 : 0.083)),
    fixation: clamp(fixation),
    faultSplit: clamp(faultSplit),
  };
}
