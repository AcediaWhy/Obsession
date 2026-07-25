export type RainCoreInput = {
  active: boolean;
  busy: boolean;
  reducedMotion: boolean;
};

export type RainCoreTransitionDrop = {
  fall: number;
  alpha: number;
};

export type RainCoreSnapshot = {
  light: number;
  water: number;
  label: "OFF" | "···" | "ON";
  transitionDrop: RainCoreTransitionDrop | null;
};

type BusyDirection = "starting" | "stopping" | null;

export class RainCoreModel {
  private light: number;
  private water: number;
  private previousActive: boolean;
  private previousBusy = false;
  private busyDirection: BusyDirection = null;
  private transitionDropTime = -1;

  constructor(initialActive: boolean) {
    this.previousActive = initialActive;
    this.light = initialActive ? 1 : 0.12;
    this.water = initialActive ? 0.45 : 0.03;
  }

  step(dt: number, input: RainCoreInput): RainCoreSnapshot {
    if (input.busy && !this.previousBusy) {
      this.busyDirection = input.active ? "stopping" : "starting";
    } else if (!input.busy && this.previousBusy) {
      this.busyDirection = null;
    }
    if (input.active && !this.previousActive && !input.reducedMotion) this.transitionDropTime = 0;

    const target = this.target(input);
    if (input.reducedMotion) {
      this.light = target.light;
      this.water = target.water;
      this.transitionDropTime = -1;
    } else {
      const elapsed = Math.max(0, Math.min(dt, 0.25));
      this.light += (target.light - this.light) * (1 - Math.exp(-elapsed * 2.2));
      this.water += (target.water - this.water) * (1 - Math.exp(-elapsed * 1.3));
      if (this.transitionDropTime >= 0) {
        this.transitionDropTime += elapsed;
        if (this.transitionDropTime >= 2) this.transitionDropTime = -1;
      }
    }

    this.previousActive = input.active;
    this.previousBusy = input.busy;
    return {
      light: this.light,
      water: this.water,
      label: input.busy ? "···" : input.active ? "ON" : "OFF",
      transitionDrop: this.transitionDropTime < 0
        ? null
        : {
            fall: Math.min(1, this.transitionDropTime / 1.1),
            alpha: this.transitionDropTime <= 1.1
              ? 1
              : Math.max(0, 1 - (this.transitionDropTime - 1.1) / 0.9),
          },
    };
  }

  private target(input: RainCoreInput): { light: number; water: number } {
    if (input.busy) {
      return this.busyDirection === "stopping"
        ? { light: 0.42, water: 0.18 }
        : { light: 0.58, water: 0.28 };
    }
    return input.active ? { light: 1, water: 0.45 } : { light: 0.12, water: 0.03 };
  }
}

