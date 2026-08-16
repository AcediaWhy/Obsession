import { describe, expect, it } from "vitest";

import {
  FrameScheduler,
  classifyRefreshRate,
  chooseCompatibleCadence,
  chooseTargetCadence,
  frameQualityScale,
  type FrameHost,
} from "./frameScheduler";

class FakeFrameHost implements FrameHost {
  private time = 0;
  private nextId = 1;
  private callbacks = new Map<number, (now: number) => void>();

  now = () => this.time;

  requestFrame = (callback: (now: number) => void) => {
    const id = this.nextId++;
    this.callbacks.set(id, callback);
    return id;
  };

  cancelFrame = (id: number) => {
    this.callbacks.delete(id);
  };

  step(deltaMs: number) {
    this.time += deltaMs;
    const callbacks = [...this.callbacks.values()];
    this.callbacks.clear();
    callbacks.forEach((callback) => callback(this.time));
  }

  consume(ms: number) {
    this.time += ms;
  }

  get pendingFrames() {
    return this.callbacks.size;
  }
}

function stepFrames(host: FakeFrameHost, hz: number, count: number) {
  for (let index = 0; index < count; index += 1) {
    host.step(1000 / hz);
  }
}

function recordWindow(
  scheduler: FrameScheduler,
  startAt: number,
  intervalMs: number,
  costMs: number,
  count = 4,
) {
  for (let index = 0; index < count; index += 1) {
    scheduler.recordFrameSample(intervalMs, costMs, startAt + index);
  }
}

describe("refresh classification and cadence", () => {
  it("classifies common refresh rates despite timestamp jitter", () => {
    expect(classifyRefreshRate([16.7, 16.5, 16.8, 16.6, 16.7])).toBe(60);
    expect(classifyRefreshRate([6.8, 7.0, 6.9, 7.1, 6.9])).toBe(144);
    expect(classifyRefreshRate([4.1, 4.2, 4.15, 4.2, 4.1])).toBe(240);
  });

  it("selects cadences compatible with 60/120/144/165/240 Hz", () => {
    expect(chooseTargetCadence(60, "high")).toBe(60);
    expect(chooseTargetCadence(120, "high")).toBe(120);
    expect(chooseTargetCadence(144, "high")).toBe(144);
    expect(chooseTargetCadence(165, "high")).toBe(165);
    expect(chooseTargetCadence(240, "high")).toBe(120);
    expect(chooseTargetCadence(144, "balanced")).toBe(72);
    expect(chooseTargetCadence(240, "balanced")).toBe(120);

    expect(chooseCompatibleCadence(144, 60)).toBe(72);
    expect(chooseCompatibleCadence(165, 60)).toBe(55);
    expect(chooseCompatibleCadence(240, 60)).toBe(60);
  });
});

describe("FrameScheduler", () => {
  it("uses two stable measurement windows before changing refresh", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 60,
      refreshSampleSize: 4,
      refreshConfirmations: 2,
    });
    const loop = scheduler.createLoop(() => {});
    loop.start();

    stepFrames(host, 144, 5);
    expect(scheduler.getSnapshot().refreshHz).toBe(60);
    stepFrames(host, 144, 4);
    expect(scheduler.getSnapshot().refreshHz).toBe(144);

    stepFrames(host, 120, 4);
    expect(scheduler.getSnapshot().refreshHz).toBe(144);
    stepFrames(host, 120, 4);
    expect(scheduler.getSnapshot().refreshHz).toBe(120);
  });

  it("fully stops while hidden and draws one frame for reduced motion", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 60,
      telemetryWindowSize: 1,
    });
    let draws = 0;
    const loop = scheduler.createLoop(() => {
      draws += 1;
    });

    loop.start();
    scheduler.setRenderState({ hidden: true, reducedMotion: false });
    expect(host.pendingFrames).toBe(0);
    host.step(16.7);
    expect(draws).toBe(0);

    scheduler.setRenderState({ hidden: false, reducedMotion: false });
    host.step(16.7);
    expect(draws).toBe(1);

    scheduler.setRenderState({ hidden: false, reducedMotion: true });
    host.step(16.7);
    expect(draws).toBe(2);
    expect(host.pendingFrames).toBe(0);

    loop.invalidate();
    host.step(1_000);
    expect(draws).toBe(3);
    expect(host.pendingFrames).toBe(0);
    expect(scheduler.getSnapshot().telemetry.samples).toBe(0);
  });

  it("resumes after a long frame gap without advancing animation or telemetry", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 60,
      telemetryWindowSize: 1,
      qualityGraceMs: 0,
    });
    const deltas: number[] = [];
    scheduler.createLoop((dt) => deltas.push(dt)).start();

    host.step(1000 / 60);
    host.step(120_000);

    expect(deltas).toHaveLength(2);
    expect(deltas[1]).toBeCloseTo(1 / 60, 4);
    expect(scheduler.getSnapshot().qualityTier).toBe("high");
    expect(scheduler.getSnapshot().telemetry.samples).toBe(0);

    host.step(1000 / 60);
    expect(scheduler.getSnapshot().telemetry.samples).toBe(1);
  });

  it("applies quality hysteresis and cooldown without tier flapping", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 120,
      telemetryWindowSize: 4,
      qualityCooldownMs: 1_000,
      qualityUpgradeWindows: 2,
      qualityGraceMs: 0,
    });

    recordWindow(scheduler, 0, 25, 12);
    expect(scheduler.getSnapshot().qualityTier).toBe("balanced");

    recordWindow(scheduler, 500, 25, 12);
    expect(scheduler.getSnapshot().qualityTier).toBe("balanced");

    recordWindow(scheduler, 1_100, 25, 12);
    expect(scheduler.getSnapshot().qualityTier).toBe("low");

    recordWindow(scheduler, 2_200, 8.3, 1);
    expect(scheduler.getSnapshot().qualityTier).toBe("low");
    recordWindow(scheduler, 2_300, 8.3, 1);
    expect(scheduler.getSnapshot().qualityTier).toBe("balanced");
  });

  it("publishes bounded telemetry instead of notifying on every frame", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 60,
      telemetryWindowSize: 4,
    });
    let notifications = 0;
    scheduler.subscribe(() => {
      notifications += 1;
    });

    recordWindow(scheduler, 0, 16.7, 2, 3);
    expect(notifications).toBe(0);
    scheduler.recordFrameSample(33.4, 5, 4);

    const snapshot = scheduler.getSnapshot();
    expect(notifications).toBe(1);
    expect(snapshot.telemetry.samples).toBe(4);
    expect(snapshot.telemetry.droppedFrames).toBeGreaterThan(0);
    expect(snapshot.telemetry.p95FrameIntervalMs).toBeGreaterThanOrEqual(16.7);
    expect(snapshot.telemetry.intervalHistogram.reduce((sum, count) => sum + count, 0)).toBe(4);
  });

  it("schedules field and preview workloads at distinct compatible cadences", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, { initialRefreshHz: 144 });
    let fieldDraws = 0;
    let previewDraws = 0;
    scheduler.createLoop(() => {
      fieldDraws += 1;
    }, { role: "field" }).start();
    scheduler.createLoop(() => {
      previewDraws += 1;
    }, { role: "preview" }).start();

    stepFrames(host, 144, 10);

    expect(fieldDraws).toBe(10);
    expect(previewDraws).toBe(5);
  });

  it("keeps the startup grace: no downgrades from boot jank, then reacts", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 120,
      telemetryWindowSize: 4,
      qualityCooldownMs: 1_000,
      qualityGraceMs: 3_000,
    });

    // Джанк первых секунд (компиляция шейдеров, прогрев) — тир не падает.
    recordWindow(scheduler, 0, 25, 12);
    expect(scheduler.getSnapshot().qualityTier).toBe("high");
    recordWindow(scheduler, 1_500, 25, 12);
    expect(scheduler.getSnapshot().qualityTier).toBe("high");

    // После форы честная перегрузка приводит к даунгрейду.
    recordWindow(scheduler, 3_200, 25, 12);
    expect(scheduler.getSnapshot().qualityTier).toBe("balanced");
  });

  it("starts from a persisted tier when provided", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 60,
      initialQualityTier: "balanced",
    });
    expect(scheduler.getSnapshot().qualityTier).toBe("balanced");
    expect(frameQualityScale(scheduler.getSnapshot().qualityTier)).toBe(0.8);
  });

  it("delivers quality changes to backing-store callbacks only on tier changes", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, {
      initialRefreshHz: 120,
      telemetryWindowSize: 4,
      qualityCooldownMs: 1_000,
      qualityGraceMs: 0,
    });
    const quality: string[] = [];
    scheduler.createLoop(() => {}, {
      role: "field",
      onQualityChange: (tier) => quality.push(tier),
    }).start();

    host.step(1000 / 120);
    host.step(1000 / 120);
    expect(quality).toEqual(["high"]);

    recordWindow(scheduler, 100, 25, 12);
    host.step(1000 / 120);
    host.step(1000 / 120);

    expect(quality).toEqual(["high", "balanced"]);
    expect(frameQualityScale("high")).toBe(1);
    expect(frameQualityScale("balanced")).toBe(0.8);
    expect(frameQualityScale("low")).toBe(0.65);
  });

  it("keeps explicit loop fps compatible with the measured display", () => {
    const host = new FakeFrameHost();
    const scheduler = new FrameScheduler(host, { initialRefreshHz: 144 });
    let draws = 0;
    const loop = scheduler.createLoop(() => {
      draws += 1;
      host.consume(0.2);
    }, { fps: 60 });
    loop.start();

    stepFrames(host, 144, 10);

    expect(draws).toBe(5);
  });
});
