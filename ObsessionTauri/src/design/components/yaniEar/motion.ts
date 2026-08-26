import type { EarPose, EarSpring, EarSpringPose, EarTargetInput } from "./types";

function clamp(value: number, low: number, high: number): number {
  return Math.max(low, Math.min(high, Number.isFinite(value) ? value : 0));
}

function hash(value: number): number {
  const x = Math.sin(value * 91.3458 + 17.217) * 47453.5453;
  return x - Math.floor(x);
}

function pulse(time: number, period: number, width: number, seed: number): number {
  const cycle = Math.floor(time / period);
  let result = 0;
  for (let offset = -1; offset <= 1; offset += 1) {
    const index = cycle + offset;
    const center = (index + 0.5) * period + (hash(index * 31 + seed) - 0.5) * period * 0.28;
    const distance = Math.abs(time - center) / width;
    if (distance < 1) result = Math.max(result, 0.5 + 0.5 * Math.cos(distance * Math.PI));
  }
  return result;
}

export function earTarget(input: EarTargetInput): EarPose {
  const time = Math.max(0, Number.isFinite(input.time) ? input.time : 0);
  const pointerX = clamp(input.pointerX, -1, 1);
  const pointerY = clamp(input.pointerY, -1, 1);
  const pointerActive = clamp(input.pointerActive, 0, 1);
  const side = input.side;
  const nearGain = pointerX * side > 0 ? 1 : 0.58;
  const twitch = pulse(time, side < 0 ? 5.9 : 7.1, 0.16, side < 0 ? 13 : 29);
  const scanStep = Math.floor(time * 2.8 + (side < 0 ? 0 : 0.63));
  const scanYaw = (hash(scanStep * 17 + (side < 0 ? 3 : 11)) - 0.5) * 0.34;
  const scanPitch = (hash(scanStep * 23 + (side < 0 ? 5 : 19)) - 0.5) * 0.16;
  const breathing = Math.sin(time * 0.82 + side * 0.31) * 0.012;
  const alarm = input.mood === "alarm" ? 1 : 0;
  const scanning = input.mood === "scanning" ? 1 : 0;
  const busy = input.mood === "busy" ? 1 : 0;
  const active = input.mood === "active" ? 1 : 0;

  const trackedYaw = -pointerX * (0.12 + nearGain * 0.12) * pointerActive;
  const trackedPitch = pointerY * (0.055 + nearGain * 0.045) * pointerActive;
  const yaw = alarm
    ? side * 0.16
    : scanning
      ? scanYaw + trackedYaw * 0.72
      : trackedYaw + side * twitch * 0.075;

  return {
    yaw,
    pitch: alarm ? 0.72 : trackedPitch - busy * 0.09 + active * 0.025 + scanPitch * scanning,
    splay: side * (0.11 + alarm * 0.34 - busy * 0.055 + twitch * 0.055),
    cup: clamp(0.06 + busy * 0.14 + scanning * 0.1 - alarm * 0.12 + nearGain * pointerActive * 0.06, -0.2, 0.3),
    tip: side * (twitch * 0.14 + scanning * scanYaw * 0.22 - alarm * 0.12),
    lift: breathing + busy * 0.025 + active * 0.012 - alarm * 0.055,
    ringSwing: -yaw * 0.65 + side * twitch * 0.12,
  };
}

export function stepEarSpring(
  spring: EarSpring,
  target: number,
  dt: number,
  frequency = 8.5,
  damping = 0.78,
): EarSpring {
  if (!Number.isFinite(dt) || dt <= 0) return spring;
  const safeDt = Math.min(dt, 1 / 20);
  const stiffness = frequency * frequency;
  const drag = 2 * damping * frequency;
  const acceleration = stiffness * (target - spring.value) - drag * spring.velocity;
  const velocity = spring.velocity + acceleration * safeDt;
  return { value: spring.value + velocity * safeDt, velocity };
}

export function createEarSpringPose(pose: EarPose): EarSpringPose {
  return Object.fromEntries(
    (Object.keys(pose) as (keyof EarPose)[]).map((key) => [key, { value: pose[key], velocity: 0 }]),
  ) as EarSpringPose;
}

export function stepEarSpringPose(current: EarSpringPose, target: EarPose, dt: number): EarSpringPose {
  const next = {} as EarSpringPose;
  for (const key of Object.keys(target) as (keyof EarPose)[]) {
    const frequency = key === "ringSwing" ? 5.6 : key === "tip" ? 11.5 : 8.2;
    const damping = key === "ringSwing" ? 0.48 : key === "tip" ? 0.62 : 0.8;
    next[key] = stepEarSpring(current[key], target[key], dt, frequency, damping);
  }
  return next;
}
