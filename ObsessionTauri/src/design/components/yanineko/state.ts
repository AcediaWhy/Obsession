import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import type { YaniMood, YaniSignals } from "./types";

export function deriveYaniMood(signals: YaniSignals): YaniMood {
  if (signals.alarm) return "alarm";
  if (signals.scanning) return "scanning";
  if (signals.busy) return "busy";
  if (signals.active) return "active";
  return "idle";
}
export function yaniMoodForPhase(phase: ObsessionVisualPhase): YaniMood {
  if (phase === "fault") return "alarm";
  if (phase === "scanning") return "scanning";
  if (phase === "engaging") return "busy";
  if (phase === "focused") return "active";
  return "idle";
}

export function yaniMoodValue(mood: YaniMood): number {
  if (mood === "busy") return 1;
  if (mood === "scanning") return 2;
  if (mood === "active") return 3;
  if (mood === "alarm") return 4;
  return 0;
}
