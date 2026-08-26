import { yaniSignedHash } from "./random";
import type { YaniMotionFrame, YaniMotionInput } from "./types";

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
}

function smoothstep(edge0: number, edge1: number, value: number): number {
  const t = clamp01((value - edge0) / Math.max(0.0001, edge1 - edge0));
  return t * t * (3 - 2 * t);
}

/** A deterministic event pulse. Jitter is bounded so neighbouring windows never reorder. */
export function yaniEventPulse(
  time: number,
  period: number,
  jitter: number,
  width: number,
  seed: number,
): number {
  if (!Number.isFinite(time) || period <= 0 || width <= 0) return 0;
  const cycle = Math.floor(time / period);
  let result = 0;
  for (let offset = -1; offset <= 1; offset += 1) {
    const index = cycle + offset;
    const center = (index + 0.5) * period + yaniSignedHash(index * 131 + seed) * jitter * 0.5;
    const distance = Math.abs(time - center) / width;
    if (distance < 1) result = Math.max(result, 0.5 + 0.5 * Math.cos(distance * Math.PI));
  }
  return result;
}

export function sampleYaniMotion(input: YaniMotionInput): YaniMotionFrame {
  const time = Number.isFinite(input.time) ? Math.max(0, input.time) : 0;
  const age = Number.isFinite(input.moodAge) ? Math.max(0, input.moodAge) : 0;
  const hover = clamp01(input.hover ?? 0);
  const pointerX = Math.max(-1, Math.min(1, input.pointerX ?? 0));
  const pointerY = Math.max(-1, Math.min(1, input.pointerY ?? 0));

  const blink = yaniEventPulse(time, 5.35, 1.15, 0.16, 17);
  const earTwitch = yaniEventPulse(time, 3.9, 0.9, 0.28, 53);
  const ringGlint = yaniEventPulse(time, 8.7, 1.4, 0.52, 91);
  const saccadeCycle = Math.floor(time / 1.7);
  const idleGazeX = yaniSignedHash(saccadeCycle * 19 + 7) * 0.34;
  const idleGazeY = yaniSignedHash(saccadeCycle * 23 + 11) * 0.18;
  const breath = 0.5 + 0.5 * Math.sin(time * 1.34 - 0.7);
  const inhale = input.mood === "busy"
    ? smoothstep(0, 0.34, age) * (1 - smoothstep(0.75, 1.35, age))
    : 0;
  const exhale = input.mood === "active"
    ? smoothstep(0, 0.8, age) * (1 - 0.2 * smoothstep(2.4, 5, age))
    : 0;
  const scanning = input.mood === "scanning" ? 1 : 0;
  const alarm = input.mood === "alarm" ? 1 : 0;
  const active = input.mood === "active" ? 1 : 0;

  const scanStep = Math.floor(time * 4.2);
  const scanX = yaniSignedHash(scanStep * 31 + 3) * 0.82;
  const scanY = yaniSignedHash(scanStep * 37 + 5) * 0.38;
  const alarmJitter = Math.sin(time * 23) * 0.15 + Math.sin(time * 37 + 1.4) * 0.07;
  const gazeX = alarm
    ? alarmJitter
    : scanning
      ? scanX
      : idleGazeX * (1 - hover) + pointerX * hover * 0.72;
  const gazeY = alarm
    ? -0.12 + Math.sin(time * 17) * 0.05
    : scanning
      ? scanY
      : idleGazeY * (1 - hover) + pointerY * hover * 0.42;

  const baseEye = input.mood === "idle" ? 0.68 : active ? 0.82 : 0.76;
  const eyeOpen = clamp01(
    alarm ? 0.18 + 0.08 * Math.sin(time * 19) : scanning ? 1 : baseEye - blink * 0.96 - inhale * 0.28,
  );
  const pupilScale = clamp01(alarm ? 0.18 : scanning ? 0.24 : 0.55 - active * 0.12 + hover * 0.08);
  const leftAttention = hover * (pointerX < 0 ? 1 : 0.45);
  const rightAttention = hover * (pointerX >= 0 ? 1 : 0.45);

  return {
    gazeX: Math.max(-1, Math.min(1, gazeX)),
    gazeY: Math.max(-1, Math.min(1, gazeY)),
    eyeOpen,
    pupilScale,
    leftEar: clamp01(0.35 + active * 0.25 + scanning * 0.45 + leftAttention * 0.35 + earTwitch * 0.18),
    rightEar: clamp01(0.3 + active * 0.22 + scanning * 0.38 + rightAttention * 0.35),
    earFlat: clamp01(alarm * 0.95 + inhale * 0.22),
    blink,
    breath,
    hairLift: clamp01(0.18 + scanning * 0.25 + hover * 0.12 + breath * 0.08),
    cigaretteTremor: clamp01(0.08 + scanning * 0.2 + alarm * (0.58 + 0.3 * Math.abs(Math.sin(time * 27)))),
    ember: clamp01(0.3 + active * 0.38 + inhale * 0.55 + 0.08 * Math.sin(time * 2.1) - alarm * 0.28),
    smoke: clamp01(0.32 + active * 0.38 + exhale * 0.28 + scanning * 0.16 - alarm * 0.18),
    blush: clamp01(0.08 + inhale * 0.3 + alarm * 0.74),
    grimace: clamp01(alarm * 0.94 + inhale * 0.22 + 0.04 * Math.sin(time * 3.1)),
    ringGlint,
  };
}

export function smoothYaniMotion(
  current: YaniMotionFrame,
  target: YaniMotionFrame,
  dt: number,
  response = 9,
): YaniMotionFrame {
  if (!Number.isFinite(dt) || dt <= 0) return current;
  const alpha = 1 - Math.exp(-Math.min(dt, 0.25) * response);
  const next = {} as YaniMotionFrame;
  for (const key of Object.keys(current) as (keyof YaniMotionFrame)[]) {
    next[key] = current[key] + (target[key] - current[key]) * alpha;
  }
  return next;
}
