import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { LegacyReliabilityPhase } from "../../lib/tauri";
import {
  getLegacyReliabilityDisplayModel,
  LegacyReliabilityPanelView,
} from "./LegacyReliabilityPanel";

describe("LegacyReliabilityPanel", () => {
  it.each<
    [LegacyReliabilityPhase, string, "muted" | "ok" | "warn" | "danger"]
  >([
    ["inactive", "Ожидание запуска", "muted"],
    ["starting", "Запуск наблюдения", "muted"],
    ["observing", "Наблюдение", "ok"],
    ["degraded", "Наблюдение ограничено", "warn"],
    ["blind", "Наблюдение недоступно", "danger"],
  ])("maps %s to %s with %s tone", (phase, label, tone) => {
    expect(getLegacyReliabilityDisplayModel(phase)).toEqual({ label, tone });
  });

  it("renders an honest read-only observe-only capability", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView phase="observing" />,
    );

    expect(markup).toContain("Контроль надёжности");
    expect(markup).toContain("Только наблюдение");
    expect(markup).toContain("Сбои фиксируются, конфигурации не меняются.");
    expect(markup).not.toContain("Авто-восстановление");
    expect(markup).not.toContain("Стратегия");
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain('role="switch"');
  });
});
