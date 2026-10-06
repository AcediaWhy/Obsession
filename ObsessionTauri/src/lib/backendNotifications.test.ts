import { describe, expect, it } from "vitest";
import type { LogEvent } from "./tauri";
import { backendErrorNotification } from "./backendNotifications";

function event(message: string, source = "dpi", level: LogEvent["level"] = "error"): LogEvent {
  return { message, source, level, ts: "17:07:28" };
}

describe("backendErrorNotification", () => {
  it("keeps unrelated engine failures visible", () => {
    expect(backendErrorNotification(event("winws завершился с кодом 1", "dpi"))).toBe("winws завершился с кодом 1");
    expect(backendErrorNotification(event("Не удалось вернуть bundled-профиль после сброса: ошибка"))).toContain("Не удалось вернуть");
  });

  it("does not promote informational logs to notifications", () => {
    expect(backendErrorNotification(event("Отчёт измерения сохранён: C:\\report.jsonl", "dpi", "info"))).toBeNull();
    expect(backendErrorNotification(event("Проверяем конфигурацию", "dpi", "warn"))).toBeNull();
  });
});
