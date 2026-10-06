import type { LogEvent } from "./tauri";

export function backendErrorNotification(event: LogEvent): string | null {
  return event.level === "error" ? event.message : null;
}
