import type { Zapret2PickerProgress } from "../../../lib/tauri";

export type HelixStage = "searching" | "confirming" | "restoring" | "cancelling" | "success" | "finished" | "cancelled";
export type HelixState = { stage: HelixStage; candidate: string; motionOff: boolean };

export function helixStage(progress: Zapret2PickerProgress, cancelling: boolean, ending: boolean,
  confirmed: boolean, cancelled: boolean): HelixStage {
  if (ending) return cancelled || cancelling ? "cancelled" : confirmed ? "success" : "finished";
  if (cancelling) return "cancelling";
  return progress.phase === "confirmation" ? "confirming" : progress.phase === "restoring" ? "restoring" : "searching";
}
