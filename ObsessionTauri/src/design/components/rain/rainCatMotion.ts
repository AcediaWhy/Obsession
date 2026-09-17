import { rainCatPose, type RainCatPose } from '../../../labs/rainCatRig.js';
import type { ObsessionVisualPhase } from '../../obsessionVisualState';

/** Continuous transitions without replaying fault on unrelated React updates. */
export class RainCatMotion {
  time = 0;
  private phase: ObsessionVisualPhase;
  private stateAt = 0;
  private transitionAt = -1;
  private from: RainCatPose;
  private current: RainCatPose;

  constructor(phase: ObsessionVisualPhase) {
    this.phase = phase;
    this.current = rainCatPose(0, { state: phase });
    this.from = this.current;
  }

  setPhase(phase: ObsessionVisualPhase) {
    if (phase === this.phase) return;
    this.from = { ...this.current };
    this.phase = phase;
    this.stateAt = this.time;
    this.transitionAt = this.time;
  }

  step(dt: number, still = false): RainCatPose {
    if (still) {
      // Stable, recognisable state poster; no blink or transient startle.
      const poster = rainCatPose(2, { state: this.phase });
      this.current = { ...poster, blink: 0 };
      this.from = this.current;
      this.transitionAt = -1;
      return this.current;
    }
    this.time += Math.max(0, Math.min(dt, 0.25));
    const target = rainCatPose(this.time, { state: this.phase, stateAt: this.stateAt });
    const t = this.transitionAt < 0 ? 1 : Math.min(1, (this.time-this.transitionAt)/0.3);
    if (t === 1) {
      this.current = target;
      return target;
    }
    const weight = t*t*(3-2*t);
    this.current = { ...target };
    for (const key of Object.keys(target) as (keyof RainCatPose)[]) {
      this.current[key] = this.from[key]+(target[key]-this.from[key])*weight;
    }
    return this.current;
  }
}
