import type {
  ObsessionChoirMotionInput,
  ObsessionChoirMotionSample,
} from "./types";

const clamp = (value: number, low = 0, high = 1) =>
  Math.max(low, Math.min(high, value));
const fract = (value: number) => value - Math.floor(value);
const hash = (index: number, salt: number) =>
  fract(Math.sin(index * 127.1 + salt * 311.7) * 43758.5453123);

function smoothstep(low: number, high: number, value: number): number {
  const x = clamp((value - low) / Math.max(0.00001, high - low));
  return x * x * (3 - 2 * x);
}

function pulse(value: number, start: number, peak: number, end: number): number {
  return smoothstep(start, peak, value) * (1 - smoothstep(peak, end, value));
}

export type ChoirRitualWindow = { start: number; duration: number };
export type ChoirBlinkWindow = { start: number; duration: number; double: boolean };

export function choirBlinkWindow(cycle: number): ChoirBlinkWindow {
  return {
    start: cycle * 5.5 + (hash(cycle, 2.64) * 2 - 1) * 0.85,
    duration: 0.14 + hash(cycle, 7.31) * 0.07,
    double: hash(cycle, 11.93) > 0.72,
  };
}

export function choirBlinkAt(time: number): number {
  const safeTime = Number.isFinite(time) ? Math.max(0, time) : 0;
  const approximate = Math.floor(safeTime / 5.5);
  let blink = 0;
  for (let cycle = Math.max(0, approximate - 1); cycle <= approximate + 1; cycle += 1) {
    const window = choirBlinkWindow(cycle);
    const local = safeTime - window.start;
    blink = Math.max(blink, pulse(local, 0, window.duration * 0.43, window.duration));
    if (window.double) {
      const second = local - window.duration - 0.11;
      blink = Math.max(
        blink,
        pulse(second, 0, window.duration * 0.38, window.duration * 0.88),
      );
    }
  }
  return clamp(blink);
}

export function choirRitualWindow(cycle: number): ChoirRitualWindow {
  return {
    start: cycle * 18 + (hash(cycle, 4.72) * 2 - 1) * 2,
    duration: 1.1 + hash(cycle, 9.18) * 0.4,
  };
}

export function choirRitualAt(time: number): number {
  const safeTime = Number.isFinite(time) ? Math.max(0, time) : 0;
  const approximate = Math.floor(safeTime / 18);
  let ritual = 0;
  for (let cycle = Math.max(0, approximate - 1); cycle <= approximate + 1; cycle += 1) {
    const window = choirRitualWindow(cycle);
    const local = safeTime - window.start;
    ritual = Math.max(
      ritual,
      pulse(local, 0, Math.min(0.24, window.duration * 0.25), window.duration),
    );
  }
  return clamp(ritual);
}

export function sampleObsessionChoirMotion({
  time,
  phase,
  phaseAge,
  forceRitual = false,
}: ObsessionChoirMotionInput): ObsessionChoirMotionSample {
  const safeTime = Number.isFinite(time) ? Math.max(0, time) : 0;
  const safeAge = Number.isFinite(phaseAge) ? Math.max(0, phaseAge) : 0;
  const breathing = 0.5 + 0.5 * Math.sin(safeTime * 0.43 + Math.sin(safeTime * 0.13) * 0.8);
  const ritual = forceRitual ? 1 : phase === "idle" ? choirRitualAt(safeTime) : 0;
  const saccadeStep = 1.85;
  const saccadeIndex = Math.floor(safeTime / saccadeStep);
  const saccadeBlend = smoothstep(0.72, 0.98, fract(safeTime / saccadeStep));
  const saccade = (salt: number, amplitude: number) => {
    const current = (hash(saccadeIndex, salt) * 2 - 1) * amplitude;
    const next = (hash(saccadeIndex + 1, salt) * 2 - 1) * amplitude;
    return current + (next - current) * saccadeBlend;
  };
  const idleBlink = phase === "idle" || phase === "focused" ? choirBlinkAt(safeTime) : 0;
  let masterGazeX = saccade(3.14, 0.42) + Math.sin(safeTime * 0.67) * 0.035;
  let masterGazeY = saccade(6.27, 0.26) + Math.cos(safeTime * 0.53 + 0.6) * 0.025;
  let masterLidOpen = (0.82 + breathing * 0.13) * (1 - idleBlink * 0.94);
  let masterBodyX = 0.5
    + Math.sin(safeTime * 0.19 + 0.4) * 0.22
    + Math.sin(safeTime * 0.057) * 0.07;
  let masterBodyY = 0.5
    + Math.cos(safeTime * 0.17 + 1.1) * 0.19
    + Math.sin(safeTime * 0.071) * 0.06;
  let masterRoll = 0.5 + Math.sin(safeTime * 0.12 - 0.7) * 0.28;
  let masterPulse = 0.3 + breathing * 0.58;
  let pupilScale = 0.38 + breathing * 0.38 + idleBlink * 0.16;
  let chorusReveal = 0.08 + breathing * 0.055 + ritual * 0.82;
  let chorusAlignment = ritual;
  let lineTension = 0.42 + breathing * 0.34;
  let lineFlow = 0.45 + breathing * 0.3;
  let apertureOpen = 0.72 + breathing * 0.12;
  let carmineDepth = 0.42 + breathing * 0.24 + ritual * 0.2;
  let faultShear = 0;

  if (ritual > 0) {
    masterGazeX *= 1 - ritual;
    masterGazeY *= 1 - ritual;
    masterLidOpen += ritual * 0.05;
    masterBodyX += (0.5 - masterBodyX) * ritual;
    masterBodyY += (0.5 - masterBodyY) * ritual;
    masterRoll += (0.5 - masterRoll) * ritual;
    masterPulse += ritual * 0.12;
    pupilScale = 0.92;
    lineFlow += ritual * 0.24;
  }

  if (phase === "engaging") {
    const close = smoothstep(0.02, 0.22, safeAge);
    const reopen = smoothstep(0.31, 1.12, safeAge);
    apertureOpen = 0.76 - close * 0.68 + reopen * 0.76;
    masterLidOpen = 0.88 - close * 0.54 + reopen * 0.5;
    lineTension = 0.55 + close * 0.38 - reopen * 0.22;
    lineFlow = 0.64 + close * 0.34 - reopen * 0.12;
    chorusReveal = 0.12 + close * 0.2;
    carmineDepth = 0.52 + close * 0.3;
    masterPulse = 0.68 + reopen * 0.25;
    pupilScale = 0.2 + reopen * 0.52;
  } else if (phase === "scanning") {
    const node = Math.floor(safeTime / 0.52);
    masterGazeX = (hash(node, 3.1) * 2 - 1) * 0.72;
    masterGazeY = (hash(node, 7.7) * 2 - 1) * 0.42;
    masterLidOpen = 0.7 + Math.sin(safeTime * 3.7) * 0.08;
    chorusReveal = 0.38 + Math.pow(Math.max(0, Math.sin(safeTime * 6.05)), 8) * 0.5;
    chorusAlignment = 0.36 + Math.pow(Math.max(0, Math.cos(safeTime * 6.05)), 10) * 0.42;
    lineTension = 0.9;
    lineFlow = 0.96;
    apertureOpen = 0.5 + Math.sin(safeTime * 2.4) * 0.16;
    carmineDepth = 0.72;
    masterBodyX = 0.5 + masterGazeX * 0.12;
    masterBodyY = 0.5 + masterGazeY * 0.1;
    masterRoll = 0.5 + Math.sin(safeTime * 1.8) * 0.16;
    masterPulse = 0.82;
    pupilScale = 0.28 + Math.pow(Math.max(0, Math.sin(safeTime * 3.1)), 3) * 0.32;
  } else if (phase === "focused") {
    // "Focused" means the system has settled, not that the eye has turned
    // into a still image. Keep deliberate saccades, full blinks and visible
    // orbital breathing, but make them calmer than the idle hunt.
    masterGazeX = masterGazeX * 0.48 + Math.sin(safeTime * 0.31) * 0.035;
    masterGazeY = masterGazeY * 0.56 + Math.cos(safeTime * 0.27) * 0.024;
    masterLidOpen = (0.88 + Math.sin(safeTime * 0.34) * 0.035) * (1 - idleBlink * 0.97);
    chorusReveal = 0.16;
    chorusAlignment = 0.82;
    lineTension = 0.46;
    lineFlow = 0.46 + breathing * 0.08;
    apertureOpen = 0.92;
    carmineDepth = 0.78 + Math.sin(safeTime * 0.22) * 0.04;
    masterBodyX = 0.5
      + Math.sin(safeTime * 0.23) * 0.13
      + Math.sin(safeTime * 0.61 + 0.8) * 0.035
      + Math.sin(safeTime * 1.08 + 0.2) * 0.045;
    masterBodyY = 0.5
      + Math.cos(safeTime * 0.19 + 0.35) * 0.11
      + Math.sin(safeTime * 0.47) * 0.026
      + Math.sin(safeTime * 0.83 + 1.1) * 0.032;
    masterRoll = 0.5 + Math.sin(safeTime * 0.17 - 0.4) * 0.15;
    masterPulse = 0.6 + breathing * 0.19;
    pupilScale = 0.56 + breathing * 0.11 + idleBlink * 0.12;
  } else if (phase === "fault") {
    const jerk = Math.sin(safeTime * 8.7) * Math.sin(safeTime * 3.1 + 0.8);
    masterGazeX = clamp(jerk * 0.52, -1, 1);
    masterGazeY = Math.cos(safeTime * 7.3) * 0.26;
    masterLidOpen = clamp(0.6 + Math.sin(safeTime * 4.2) * 0.2, 0.24, 0.9);
    chorusReveal = 0.58;
    chorusAlignment = 0.15;
    lineTension = 1;
    lineFlow = 1;
    apertureOpen = 0.32 + Math.abs(jerk) * 0.34;
    carmineDepth = 0.86;
    faultShear = 0.48 + Math.abs(Math.sin(safeTime * 4.9)) * 0.52;
    masterBodyX = 0.5 + jerk * 0.24;
    masterBodyY = 0.5 + Math.cos(safeTime * 6.1) * 0.17;
    masterRoll = 0.5 + jerk * 0.38;
    masterPulse = 0.92;
    pupilScale = 0.28 + Math.abs(jerk) * 0.48;
  }

  return {
    masterGazeX: clamp(masterGazeX, -1, 1),
    masterGazeY: clamp(masterGazeY, -1, 1),
    masterLidOpen: clamp(masterLidOpen, 0.05, 1),
    masterBodyX: clamp(masterBodyX),
    masterBodyY: clamp(masterBodyY),
    masterRoll: clamp(masterRoll),
    masterPulse: clamp(masterPulse),
    pupilScale: clamp(pupilScale),
    chorusReveal: clamp(chorusReveal),
    chorusAlignment: clamp(chorusAlignment),
    lineTension: clamp(lineTension),
    lineFlow: clamp(lineFlow),
    apertureOpen: clamp(apertureOpen),
    carmineDepth: clamp(carmineDepth),
    faultShear: clamp(faultShear),
    ritual: clamp(ritual),
  };
}

const MOTION_CHANNELS = [
  "masterGazeX",
  "masterGazeY",
  "masterLidOpen",
  "masterBodyX",
  "masterBodyY",
  "masterRoll",
  "masterPulse",
  "pupilScale",
  "chorusReveal",
  "chorusAlignment",
  "lineTension",
  "lineFlow",
  "apertureOpen",
  "carmineDepth",
  "faultShear",
  "ritual",
] as const satisfies readonly (keyof ObsessionChoirMotionSample)[];

/**
 * Converts discrete phase samples into one continuous physical pose. The
 * exponential response is frame-rate independent and, unlike resetting a CSS
 * transition on every store update, preserves the currently visible frame.
 */
export function smoothObsessionChoirMotion(
  current: ObsessionChoirMotionSample,
  target: ObsessionChoirMotionSample,
  dt: number,
  response = 8.5,
): ObsessionChoirMotionSample {
  const safeDt = Number.isFinite(dt) ? Math.max(0, Math.min(dt, 0.25)) : 0;
  const safeResponse = Number.isFinite(response) ? Math.max(0, response) : 0;
  const alpha = clamp(1 - Math.exp(-safeDt * safeResponse));
  const next = { ...current };
  for (const channel of MOTION_CHANNELS) {
    next[channel] = current[channel] + (target[channel] - current[channel]) * alpha;
  }
  return next;
}
