export type QualityTier = "high" | "balanced" | "low";
export type FrameRole = "field" | "hero" | "preview" | "secondary";

export type FrameTelemetry = {
  samples: number;
  droppedFrames: number;
  missedFrameRatio: number;
  p50FrameIntervalMs: number;
  p95FrameIntervalMs: number;
  p99FrameIntervalMs: number;
  p95FrameCostMs: number;
  intervalHistogram: readonly number[];
};

export type FrameSchedulerSnapshot = {
  refreshHz: number;
  targetFps: number;
  qualityTier: QualityTier;
  hidden: boolean;
  reducedMotion: boolean;
  telemetry: FrameTelemetry;
};

export type FrameHost = {
  now(): number;
  requestFrame(callback: (now: number) => void): number;
  cancelFrame(id: number): void;
};

export type FrameLoop = {
  start(): void;
  stop(): void;
  setPaused(paused: boolean): void;
  invalidate(): void;
  dispose(): void;
};

export type FrameLoopOptions = {
  fps?: number;
  maxDt?: number;
  paused?: boolean;
  role?: FrameRole;
  onQualityChange?: (qualityTier: QualityTier) => void;
};

export type FrameSchedulerOptions = {
  initialRefreshHz?: number;
  refreshSampleSize?: number;
  refreshConfirmations?: number;
  telemetryWindowSize?: number;
  qualityCooldownMs?: number;
  qualityUpgradeWindows?: number;
  /** Стартовый тир (например, сохранённый с прошлой сессии). */
  initialQualityTier?: QualityTier;
  /** Стартовая фора: до её истечения даунгрейды запрещены — джанк первых
   *  секунд (компиляция шейдеров, прогрев WebView) не роняет качество. */
  qualityGraceMs?: number;
};

type FrameTask = {
  id: number;
  draw: (dt: number, now: number) => void;
  requestedFps: number;
  maxDt: number;
  role: FrameRole;
  onQualityChange?: (qualityTier: QualityTier) => void;
  appliedQuality: QualityTier | null;
  paused: boolean;
  started: boolean;
  dirty: boolean;
  disposed: boolean;
  lastDraw: number;
  nextDue: number;
};

type Listener = () => void;

const COMMON_REFRESH_RATES = [30, 48, 50, 60, 72, 75, 90, 100, 120, 144, 165, 180, 200, 240, 360];
const QUALITY_CAPS: Record<QualityTier, number> = {
  high: 180,
  balanced: 120,
  low: 60,
};
const QUALITY_ORDER: readonly QualityTier[] = ["low", "balanced", "high"];
const HISTOGRAM_LIMITS = [8, 12, 17, 25, 34, 50] as const;

const DEFAULT_OPTIONS: Required<FrameSchedulerOptions> = {
  initialRefreshHz: 60,
  refreshSampleSize: 24,
  refreshConfirmations: 2,
  telemetryWindowSize: 120,
  qualityCooldownMs: 10_000,
  qualityUpgradeWindows: 3,
  initialQualityTier: "high",
  qualityGraceMs: 5_000,
};

function roundCadence(value: number): number {
  return Math.round(value * 10) / 10;
}

function percentile(values: readonly number[], ratio: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * ratio) - 1));
  return Math.round(sorted[index] * 100) / 100;
}

function emptyTelemetry(): FrameTelemetry {
  return {
    samples: 0,
    droppedFrames: 0,
    missedFrameRatio: 0,
    p50FrameIntervalMs: 0,
    p95FrameIntervalMs: 0,
    p99FrameIntervalMs: 0,
    p95FrameCostMs: 0,
    intervalHistogram: Array.from({ length: HISTOGRAM_LIMITS.length + 1 }, () => 0),
  };
}

export function classifyRefreshRate(intervalsMs: readonly number[]): number {
  const usable = intervalsMs.filter((value) => Number.isFinite(value) && value >= 2 && value <= 50);
  if (usable.length === 0) return 60;
  const medianInterval = percentile(usable, 0.5);
  const estimated = 1000 / medianInterval;
  let nearest = COMMON_REFRESH_RATES[0];
  let distance = Math.abs(estimated - nearest);
  for (const refresh of COMMON_REFRESH_RATES.slice(1)) {
    const nextDistance = Math.abs(estimated - refresh);
    if (nextDistance < distance) {
      nearest = refresh;
      distance = nextDistance;
    }
  }
  return distance / nearest <= 0.06 ? nearest : Math.round(estimated);
}

export function chooseCompatibleCadence(refreshHz: number, preferredFps: number): number {
  if (!Number.isFinite(refreshHz) || refreshHz <= 0) return Math.max(1, preferredFps);
  if (!Number.isFinite(preferredFps) || preferredFps <= 0) return roundCadence(refreshHz);

  let best = refreshHz;
  let bestDistance = Math.abs(refreshHz - preferredFps);
  const maxDivisor = Math.max(1, Math.ceil(refreshHz / 24));
  for (let divisor = 2; divisor <= maxDivisor; divisor += 1) {
    const cadence = refreshHz / divisor;
    if (cadence < 24) break;
    const distance = Math.abs(cadence - preferredFps);
    if (distance < bestDistance - 0.001 || (Math.abs(distance - bestDistance) <= 0.001 && cadence > best)) {
      best = cadence;
      bestDistance = distance;
    }
  }
  return roundCadence(best);
}

export function chooseTargetCadence(refreshHz: number, qualityTier: QualityTier): number {
  if (!Number.isFinite(refreshHz) || refreshHz <= 0) return 60;
  const cap = QUALITY_CAPS[qualityTier];
  const divisor = Math.max(1, Math.ceil(refreshHz / cap - 0.0001));
  return roundCadence(refreshHz / divisor);
}

export function frameQualityScale(qualityTier: QualityTier): number {
  if (qualityTier === "high") return 1;
  if (qualityTier === "balanced") return 0.8;
  return 0.65;
}

export class FrameScheduler {
  private readonly host: FrameHost;
  private readonly options: Required<FrameSchedulerOptions>;
  private readonly listeners = new Set<Listener>();
  private readonly tasks = new Map<number, FrameTask>();
  private readonly refreshIntervals: number[] = [];
  private readonly telemetryIntervals: number[] = [];
  private readonly telemetryCosts: number[] = [];

  private snapshot: FrameSchedulerSnapshot;
  private nextTaskId = 1;
  private raf = 0;
  private lastRaf = 0;
  private pendingRefresh = 0;
  private pendingRefreshWindows = 0;
  private lastQualityChangeAt = Number.NEGATIVE_INFINITY;
  private healthyWindows = 0;
  private graceUntil = Number.NEGATIVE_INFINITY;

  constructor(host: FrameHost, options: FrameSchedulerOptions = {}) {
    this.host = host;
    this.options = { ...DEFAULT_OPTIONS, ...options };
    this.graceUntil = host.now() + this.options.qualityGraceMs;
    const refreshHz = this.options.initialRefreshHz;
    const qualityTier: QualityTier = this.options.initialQualityTier;
    this.snapshot = {
      refreshHz,
      targetFps: chooseTargetCadence(refreshHz, qualityTier),
      qualityTier,
      hidden: false,
      reducedMotion: false,
      telemetry: emptyTelemetry(),
    };
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = (): FrameSchedulerSnapshot => this.snapshot;

  setRenderState(state: { hidden: boolean; reducedMotion: boolean }): void {
    if (state.hidden === this.snapshot.hidden && state.reducedMotion === this.snapshot.reducedMotion) {
      return;
    }
    const wasHidden = this.snapshot.hidden;
    this.snapshot = { ...this.snapshot, ...state };
    this.resetTiming();
    if (state.hidden) {
      this.cancelScheduledFrame();
    } else {
      for (const task of this.tasks.values()) {
        if (task.started) task.dirty = true;
      }
      if (wasHidden || state.reducedMotion) this.resetRefreshMeasurement();
      this.ensureFrame();
    }
    this.notify();
  }

  createLoop(draw: (dt: number, now: number) => void, options: FrameLoopOptions = {}): FrameLoop {
    const task: FrameTask = {
      id: this.nextTaskId++,
      draw,
      requestedFps: options.fps && options.fps > 0 ? options.fps : 0,
      maxDt: options.maxDt ?? 0.25,
      role: options.role ?? "field",
      onQualityChange: options.onQualityChange,
      appliedQuality: null,
      paused: !!options.paused,
      started: false,
      dirty: false,
      disposed: false,
      lastDraw: 0,
      nextDue: 0,
    };
    this.tasks.set(task.id, task);

    return {
      start: () => {
        if (task.disposed) return;
        if (!task.started) {
          task.started = true;
          task.dirty = true;
        }
        this.ensureFrame();
      },
      stop: () => {
        if (task.disposed || !task.started) return;
        task.started = false;
        task.dirty = false;
        task.lastDraw = 0;
        task.nextDue = 0;
        this.reconcileFrame();
      },
      setPaused: (paused: boolean) => {
        if (task.disposed || paused === task.paused) return;
        task.paused = paused;
        task.dirty = true;
        if (!paused) {
          task.lastDraw = 0;
          task.nextDue = 0;
        }
        this.ensureFrame();
      },
      invalidate: () => {
        if (task.disposed || !task.started) return;
        if (task.paused || this.snapshot.reducedMotion) {
          task.dirty = true;
          this.ensureFrame();
        }
      },
      dispose: () => {
        if (task.disposed) return;
        task.disposed = true;
        this.tasks.delete(task.id);
        this.reconcileFrame();
      },
    };
  }

  recordFrameSample(intervalMs: number, costMs: number, now = this.host.now()): void {
    if (!Number.isFinite(intervalMs) || intervalMs <= 0) return;
    this.telemetryIntervals.push(intervalMs);
    this.telemetryCosts.push(Math.max(0, costMs));
    if (this.telemetryIntervals.length < this.options.telemetryWindowSize) return;

    const intervals = this.telemetryIntervals.splice(0, this.options.telemetryWindowSize);
    const costs = this.telemetryCosts.splice(0, this.options.telemetryWindowSize);
    const idealInterval = 1000 / this.snapshot.refreshHz;
    const histogram = Array.from({ length: HISTOGRAM_LIMITS.length + 1 }, () => 0);
    let droppedFrames = 0;
    for (const interval of intervals) {
      droppedFrames += Math.max(0, Math.round(interval / idealInterval) - 1);
      const bucket = HISTOGRAM_LIMITS.findIndex((limit) => interval <= limit);
      histogram[bucket >= 0 ? bucket : histogram.length - 1] += 1;
    }

    const telemetry: FrameTelemetry = {
      samples: intervals.length,
      droppedFrames,
      missedFrameRatio: droppedFrames / Math.max(1, intervals.length + droppedFrames),
      p50FrameIntervalMs: percentile(intervals, 0.5),
      p95FrameIntervalMs: percentile(intervals, 0.95),
      p99FrameIntervalMs: percentile(intervals, 0.99),
      p95FrameCostMs: percentile(costs, 0.95),
      intervalHistogram: histogram,
    };
    const qualityTier = this.evaluateQuality(telemetry, now);
    const qualityChanged = qualityTier !== this.snapshot.qualityTier;
    this.snapshot = {
      ...this.snapshot,
      qualityTier,
      targetFps: chooseTargetCadence(this.snapshot.refreshHz, qualityTier),
      telemetry,
    };
    if (qualityChanged) {
      for (const task of this.tasks.values()) {
        if (task.started) task.dirty = true;
      }
      this.ensureFrame();
    }
    this.notify();
  }

  private notify(): void {
    this.listeners.forEach((listener) => listener());
  }

  private resetTiming(): void {
    this.lastRaf = 0;
    for (const task of this.tasks.values()) {
      task.lastDraw = 0;
      task.nextDue = 0;
    }
  }

  private resetRefreshMeasurement(): void {
    this.refreshIntervals.length = 0;
    this.pendingRefresh = 0;
    this.pendingRefreshWindows = 0;
  }

  private evaluateQuality(telemetry: FrameTelemetry, now: number): QualityTier {
    const targetInterval = 1000 / this.snapshot.targetFps;
    const displayInterval = 1000 / this.snapshot.refreshHz;
    const overloaded =
      telemetry.missedFrameRatio > 0.08 ||
      telemetry.p95FrameCostMs > targetInterval * 0.7 ||
      telemetry.p95FrameIntervalMs > displayInterval * 1.5;
    // Порог подъёма 0.45 бюджета кадра: между ним и порогом перегруза (0.7)
    // остаётся зона гистерезиса — тир не осциллирует.
    const healthy =
      telemetry.missedFrameRatio < 0.01 &&
      telemetry.p95FrameCostMs < targetInterval * 0.45 &&
      telemetry.p95FrameIntervalMs <= displayInterval * 1.25;
    const cooldownElapsed = now - this.lastQualityChangeAt >= this.options.qualityCooldownMs;
    const index = QUALITY_ORDER.indexOf(this.snapshot.qualityTier);

    if (overloaded) {
      this.healthyWindows = 0;
      // Стартовая фора: джанк первых секунд (шейдеры, прогрев) не считается.
      if (now < this.graceUntil) return this.snapshot.qualityTier;
      if (cooldownElapsed && index > 0) {
        this.lastQualityChangeAt = now;
        return QUALITY_ORDER[index - 1];
      }
      return this.snapshot.qualityTier;
    }

    if (!healthy) {
      this.healthyWindows = 0;
      return this.snapshot.qualityTier;
    }

    this.healthyWindows += 1;
    if (cooldownElapsed && this.healthyWindows >= this.options.qualityUpgradeWindows && index < QUALITY_ORDER.length - 1) {
      this.healthyWindows = 0;
      this.lastQualityChangeAt = now;
      return QUALITY_ORDER[index + 1];
    }
    return this.snapshot.qualityTier;
  }

  private observeRefresh(intervalMs: number): void {
    if (!Number.isFinite(intervalMs) || intervalMs < 2 || intervalMs > 50) return;
    this.refreshIntervals.push(intervalMs);
    if (this.refreshIntervals.length < this.options.refreshSampleSize) return;

    const measured = classifyRefreshRate(this.refreshIntervals.splice(0, this.options.refreshSampleSize));
    const closeToCurrent = Math.abs(measured - this.snapshot.refreshHz) / this.snapshot.refreshHz <= 0.03;
    if (closeToCurrent) {
      this.pendingRefresh = 0;
      this.pendingRefreshWindows = 0;
      return;
    }

    const matchesPending =
      this.pendingRefresh > 0 && Math.abs(measured - this.pendingRefresh) / this.pendingRefresh <= 0.03;
    if (matchesPending) {
      this.pendingRefreshWindows += 1;
    } else {
      this.pendingRefresh = measured;
      this.pendingRefreshWindows = 1;
    }
    if (this.pendingRefreshWindows < this.options.refreshConfirmations) return;

    const refreshHz = this.pendingRefresh;
    this.pendingRefresh = 0;
    this.pendingRefreshWindows = 0;
    for (const task of this.tasks.values()) task.nextDue = 0;
    this.snapshot = {
      ...this.snapshot,
      refreshHz,
      targetFps: chooseTargetCadence(refreshHz, this.snapshot.qualityTier),
    };
    this.notify();
  }

  private cadenceFor(task: FrameTask): number {
    if (task.requestedFps > 0) {
      return chooseCompatibleCadence(this.snapshot.refreshHz, task.requestedFps);
    }
    if (task.role === "preview" || task.role === "secondary") {
      return chooseCompatibleCadence(this.snapshot.refreshHz, Math.min(60, this.snapshot.targetFps));
    }
    return this.snapshot.targetFps;
  }

  private drawTask(task: FrameTask, now: number): void {
    if (task.appliedQuality !== this.snapshot.qualityTier) {
      task.appliedQuality = this.snapshot.qualityTier;
      task.lastDraw = 0;
      task.nextDue = 0;
      try {
        task.onQualityChange?.(this.snapshot.qualityTier);
      } catch (error) {
        task.started = false;
        console.error("FrameScheduler stopped a faulting quality callback", error);
        return;
      }
    }
    const fps = this.cadenceFor(task);
    const frameMs = fps > 0 ? 1000 / fps : 0;
    const epsilon = frameMs ? Math.min(2, frameMs * 0.25) : 0;
    if (!task.dirty && frameMs && task.nextDue && now < task.nextDue - epsilon) return;

    const dt = task.lastDraw
      ? Math.min((now - task.lastDraw) / 1000, task.maxDt)
      : (frameMs || 16.7) / 1000;
    task.lastDraw = now;
    task.nextDue = frameMs
      ? task.nextDue && task.nextDue + frameMs > now
        ? task.nextDue + frameMs
        : now + frameMs
      : 0;
    task.dirty = false;
    try {
      task.draw(dt, now);
    } catch (error) {
      task.started = false;
      console.error("FrameScheduler stopped a faulting render loop", error);
    }
  }

  private tick = (now: number): void => {
    this.raf = 0;
    if (this.snapshot.hidden) return;

    const continuous = this.hasContinuousWork();
    const intervalMs = this.lastRaf ? now - this.lastRaf : 0;
    if (continuous && intervalMs > 0) this.observeRefresh(intervalMs);
    this.lastRaf = now;

    const startedAt = this.host.now();
    for (const task of [...this.tasks.values()]) {
      if (!task.disposed && task.started && (task.dirty || (!task.paused && !this.snapshot.reducedMotion))) {
        this.drawTask(task, now);
      }
    }
    const costMs = Math.max(0, this.host.now() - startedAt);
    if (continuous && intervalMs > 0) {
      this.recordFrameSample(intervalMs, costMs, this.host.now());
    }
    if (this.hasFrameWork()) {
      this.ensureFrame();
    } else {
      this.lastRaf = 0;
    }
  };

  private hasContinuousWork(): boolean {
    if (this.snapshot.reducedMotion) return false;
    for (const task of this.tasks.values()) {
      if (task.started && !task.paused) return true;
    }
    return false;
  }

  private hasFrameWork(): boolean {
    for (const task of this.tasks.values()) {
      if (task.started && (task.dirty || (!task.paused && !this.snapshot.reducedMotion))) return true;
    }
    return false;
  }

  private ensureFrame(): void {
    if (!this.raf && !this.snapshot.hidden && this.hasFrameWork()) {
      this.raf = this.host.requestFrame(this.tick);
    }
  }

  private reconcileFrame(): void {
    if (this.hasFrameWork()) {
      this.ensureFrame();
    } else {
      this.cancelScheduledFrame();
    }
  }

  private cancelScheduledFrame(): void {
    if (!this.raf) return;
    this.host.cancelFrame(this.raf);
    this.raf = 0;
  }
}

const browserFrameHost: FrameHost = {
  now: () => (typeof performance !== "undefined" ? performance.now() : Date.now()),
  requestFrame: (callback) => {
    if (typeof requestAnimationFrame === "function") return requestAnimationFrame(callback);
    return setTimeout(() => callback(Date.now()), 16) as unknown as number;
  },
  cancelFrame: (id) => {
    if (typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(id);
    } else {
      clearTimeout(id);
    }
  },
};

// Последний стабильный тир переживает перезапуск: сильная машина стартует
// сразу с high, слабая — со своего проверенного тира (без 10с джанка на старте).
const QUALITY_TIER_KEY = "obsession.frameTier";

function readPersistedTier(): QualityTier | undefined {
  try {
    const value = localStorage.getItem(QUALITY_TIER_KEY);
    return value === "high" || value === "balanced" || value === "low" ? value : undefined;
  } catch {
    return undefined;
  }
}

const persistedInitialTier = readPersistedTier();
export const frameScheduler = new FrameScheduler(
  browserFrameHost,
  persistedInitialTier ? { initialQualityTier: persistedInitialTier } : {},
);

let persistedTier = frameScheduler.getSnapshot().qualityTier;
frameScheduler.subscribe(() => {
  const tier = frameScheduler.getSnapshot().qualityTier;
  if (tier === persistedTier) return;
  persistedTier = tier;
  try {
    localStorage.setItem(QUALITY_TIER_KEY, tier);
  } catch {
    // приватный режим/квота — некритично, начнём следующую сессию с high
  }
});
