export type ObsessionVisualPhase =
  | "idle"
  | "engaging"
  | "scanning"
  | "focused"
  | "fault";

export type ObsessionVisualSignals = {
  dpi: {
    active: boolean;
    transitioning: boolean;
    testing: boolean;
    error: string;
  };
  proxy: {
    running: boolean;
    transitioning: boolean;
    error: string;
  };
  adaptive: {
    busy: boolean;
    error: string;
    phase: string | null;
  };
  brain: {
    phase: string | null;
  };
  reliability: {
    phase: string;
    activeAttemptPhase: string | null;
    haltedCategories: readonly string[];
  };
};

const ADAPTIVE_SCANNING = new Set([
  "discovering_quic",
  "calibrating",
  "searching",
  "candidate_probe",
  "temporary_verification",
  "applying",
  "rolling_back",
]);

const BRAIN_SCANNING = new Set(["confirming", "switching"]);

export function deriveObsessionVisualPhase(
  signals: ObsessionVisualSignals,
): ObsessionVisualPhase {
  const fault =
    signals.dpi.error.length > 0 ||
    signals.proxy.error.length > 0 ||
    signals.adaptive.error.length > 0 ||
    signals.adaptive.phase === "exhausted" ||
    signals.brain.phase === "exhausted" ||
    signals.reliability.phase === "blind" ||
    signals.reliability.activeAttemptPhase === "process_failed" ||
    signals.reliability.haltedCategories.length > 0;
  if (fault) return "fault";

  const scanning =
    signals.dpi.testing ||
    signals.adaptive.busy ||
    (signals.adaptive.phase != null &&
      ADAPTIVE_SCANNING.has(signals.adaptive.phase)) ||
    (signals.brain.phase != null && BRAIN_SCANNING.has(signals.brain.phase)) ||
    signals.reliability.activeAttemptPhase != null;
  if (scanning) return "scanning";

  if (signals.dpi.transitioning || signals.proxy.transitioning) {
    return "engaging";
  }
  if (signals.dpi.active || signals.proxy.running) return "focused";
  return "idle";
}

export type ObsessionFocusPoint = { x: number; y: number };

const SCREEN_FOCUS: Record<string, ObsessionFocusPoint> = {
  // Overview leaves a deliberate quiet field below its status cards. Keep the
  // optical mark there instead of hiding its aperture under the hero panel.
  overview: { x: 0.82, y: 0.78 },
  dpi: { x: 0.82, y: 0.21 },
  ai: { x: 0.77, y: 0.28 },
  telegram: { x: 0.84, y: 0.25 },
  lists: { x: 0.75, y: 0.22 },
  profiles: { x: 0.81, y: 0.29 },
  settings: { x: 0.74, y: 0.2 },
};

export function obsessionFocusForScreen(screen: string): ObsessionFocusPoint {
  return SCREEN_FOCUS[screen] ?? SCREEN_FOCUS.overview;
}

export function obsessionPhaseValue(phase: ObsessionVisualPhase): number {
  switch (phase) {
    case "engaging":
      return 1;
    case "scanning":
      return 2;
    case "focused":
      return 3;
    case "fault":
      return 4;
    default:
      return 0;
  }
}
