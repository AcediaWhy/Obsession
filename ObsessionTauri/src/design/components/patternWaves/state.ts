import type { Zapret2PickerProgress } from "../../../lib/tauri";
import type { PatternOptions } from "./scene";

export type PatternStage = "searching" | "confirming" | "restoring" | "cancelling" | "success" | "finished" | "cancelled";
export function patternStage(progress: Zapret2PickerProgress, cancelling: boolean, ending: boolean,
  confirmed: boolean, cancelled: boolean): PatternStage {
  if (ending) return cancelled || cancelling ? "cancelled" : confirmed ? "success" : "finished";
  if (cancelling) return "cancelling";
  return progress.phase === "confirmation" || progress.phase === "composition" ? "confirming" : progress.phase === "restoring" ? "restoring" : "searching";
}
export function patternOptions(stage: PatternStage): PatternOptions {
  switch (stage) {
    case "searching": return { morph:0, speed:.35, opacity:1, paused:false, interactive:true };
    case "confirming": return { morph:.55, speed:.14, opacity:1, paused:false, interactive:true };
    case "restoring": return { morph:.2, speed:.08, opacity:.75, paused:false, interactive:true };
    case "success": return { morph:1, speed:.08, opacity:1, paused:false, interactive:true };
    case "cancelling": case "cancelled": return { morph:0, speed:0, opacity:.45, paused:true, interactive:false };
    case "finished": return { morph:0, speed:0, opacity:.65, paused:true, interactive:false };
  }
}
export function patternService(category: string): string {
  return category === "discord" ? "discord" : category === "youtube_twitch" ? "youtube" : "gaming";
}
