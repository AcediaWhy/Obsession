export type RainWeatherInput = {
  active: boolean;
  reducedMotion: boolean;
};

export type RainWeatherSnapshot = {
  activity: number;
  rainDensity: number;
  rainSpeed: number;
  trailRate: number;
  wind: number;
  lightning: number;
};

export type RainRandom = () => number;

const ACTIVITY_TIME_CONSTANT = 1.1;
const LIGHTNING_MIN_DELAY = 45;
const LIGHTNING_MAX_DELAY = 120;
const LIGHTNING_PEAK = 0.28;

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

function pulse(time: number, start: number, duration: number): number {
  if (time <= start || time >= start + duration) return 0;
  const phase = (time - start) / duration;
  return Math.sin(Math.PI * phase) ** 2;
}

export class RainWeatherModel {
  private readonly random: RainRandom;
  private activity: number;
  private nextLightningIn: number;
  private lightningTime = -1;
  private doubleLightning = false;
  private reducedMotion = false;

  constructor(active = false, random: RainRandom = Math.random) {
    this.random = random;
    this.activity = active ? 1 : 0;
    this.nextLightningIn = this.pickLightningDelay();
  }

  step(dt: number, input: RainWeatherInput): RainWeatherSnapshot {
    const elapsed = Math.max(0, Math.min(dt, 0.25));
    const target = input.active ? 1 : 0;
    if (input.reducedMotion) {
      this.activity = target;
      if (!this.reducedMotion) {
        this.lightningTime = -1;
        this.nextLightningIn = this.pickLightningDelay();
      }
    } else {
      const alpha = 1 - Math.exp(-elapsed / ACTIVITY_TIME_CONSTANT);
      this.activity += (target - this.activity) * alpha;
    }

    const lightning = input.reducedMotion ? 0 : this.stepLightning(elapsed);
    this.reducedMotion = input.reducedMotion;
    const activity = clamp01(this.activity);
    return {
      activity,
      rainDensity: 1 + activity * 0.35,
      rainSpeed: 1 + activity * 0.25,
      trailRate: 1 + activity * 0.3,
      // Ветер шторма: сносит капли по стеклу наискосок, сильнее в активной фазе.
      wind: 0.25 + activity * 0.75,
      lightning,
    };
  }

  private stepLightning(dt: number): number {
    if (this.lightningTime < 0) {
      this.nextLightningIn -= dt;
      if (this.nextLightningIn > 0) return 0;
      this.lightningTime = 0;
      this.doubleLightning = this.random() >= 0.5;
    } else {
      this.lightningTime += dt;
    }

    const value = this.doubleLightning
      ? Math.max(pulse(this.lightningTime, 0, 0.13), pulse(this.lightningTime, 0.2, 0.22))
      : pulse(this.lightningTime, 0, 0.28);
    const duration = this.doubleLightning ? 0.42 : 0.28;
    if (this.lightningTime >= duration) {
      this.lightningTime = -1;
      this.nextLightningIn = this.pickLightningDelay();
    }
    return value * LIGHTNING_PEAK;
  }

  private pickLightningDelay(): number {
    return LIGHTNING_MIN_DELAY + clamp01(this.random()) * (LIGHTNING_MAX_DELAY - LIGHTNING_MIN_DELAY);
  }
}

