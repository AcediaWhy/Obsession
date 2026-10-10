import type { ObsessionVisualPhase } from "./obsessionVisualState";

export type AtmosphereKind = "rare" | "activation" | "release";
export type AtmosphereEvent = { id: number; kind: AtmosphereKind; duration: number; variation: number };
type Host = {
  setTimeout(callback: () => void, delay: number): unknown;
  clearTimeout(id: unknown): void;
  random(): number;
};

export const ATMOSPHERE_DURATION: Record<AtmosphereKind, number> = {
  rare: 14_000, activation: 4_800, release: 3_600,
};

// A single sleeping timer between gestures; no frame loop, polling or backlog.
export function createAtmosphereController(
  host: Host,
  emit: (event: AtmosphereEvent | null) => void,
  initialPhase: ObsessionVisualPhase,
) {
  let phase = initialPhase;
  let settledPhase = initialPhase;
  let enabled = false;
  let disposed = false;
  let timer: unknown;
  let generation = 0;
  const quiet = () => phase === "idle" || phase === "focused";
  const clear = () => {
    if (timer !== undefined) host.clearTimeout(timer);
    timer = undefined;
  };
  const schedule = () => {
    clear();
    if (!enabled || disposed || !quiet()) return;
    timer = host.setTimeout(() => play("rare"), 30_000 + host.random() * 40_000);
  };
  function play(kind: AtmosphereKind) {
    if (!enabled || disposed || !quiet()) return;
    clear();
    emit({ id: ++generation, kind, duration: ATMOSPHERE_DURATION[kind], variation: host.random() });
    timer = host.setTimeout(() => {
      timer = undefined;
      emit(null);
      schedule();
    }, ATMOSPHERE_DURATION[kind]);
  }
  return {
    setEnabled(next: boolean) {
      if (disposed || enabled === next) return;
      enabled = next;
      clear();
      emit(null);
      if (enabled) schedule();
    },
    setPhase(next: ObsessionVisualPhase) {
      if (disposed || next === phase) return;
      const previousSettled = settledPhase;
      phase = next;
      // Stopping protection passes through "engaging" before reaching idle.
      if (quiet()) settledPhase = next;
      if (!enabled) return;
      if (!quiet()) {
        clear();
        emit(null);
      } else if (next === "focused") play("activation");
      else if (previousSettled === "focused") play("release");
      else schedule();
    },
    preview() { play("rare"); },
    dispose() {
      disposed = true;
      enabled = false;
      clear();
    },
  };
}
