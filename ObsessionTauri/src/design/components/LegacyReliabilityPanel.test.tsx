import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type {
  LegacyReliabilityClassification,
  LegacyReliabilityLaneAssessment,
  LegacyReliabilityPhase,
  LegacyReliabilityStatus,
} from "../../lib/tauri";
import {
  getLegacyEvidenceLabel,
  getLegacyLaneDisplayModel,
  getLegacyReliabilityDisplayModel,
  LegacyReliabilityPanelView,
} from "./LegacyReliabilityPanel";

const EMPTY_EVIDENCE = {
  workingFlows: 0,
  workingTargets: 0,
  resetFlows: 0,
  resetTargets: 0,
  blackholeFlows: 0,
  blackholeTargets: 0,
};

function lane(
  overrides: Partial<LegacyReliabilityLaneAssessment> = {},
): LegacyReliabilityLaneAssessment {
  return {
    category: "youtube_twitch",
    activeConfig: "youtube_twitch_1.conf",
    laneGeneration: 7,
    phase: "observing",
    classification: "awaiting_evidence",
    confidence: "none",
    evidence: EMPTY_EVIDENCE,
    workingConfirmedRecently: false,
    cooldownUntilMs: null,
    ...overrides,
  };
}

function status(
  overrides: Partial<LegacyReliabilityStatus> = {},
): LegacyReliabilityStatus {
  return {
    mode: "observe_only",
    phase: "observing",
    activeCategories: ["youtube_twitch"],
    sessionId: 11,
    sensorGeneration: 3,
    lanes: [lane()],
    presumedIntent: {
      kind: "wait",
      reason: "awaiting_evidence",
    },
    ...overrides,
  };
}

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

  it.each<
    [
      LegacyReliabilityClassification,
      string,
      "muted" | "ok" | "warn" | "danger",
    ]
  >([
    ["awaiting_evidence", "Сбор данных", "muted"],
    ["working", "Работает", "ok"],
    ["dpi_suspected", "Вероятна блокировка", "warn"],
    ["dpi_blocked", "Блокировка подтверждена", "danger"],
    ["offline", "Нет подключения", "warn"],
    ["dns_failure", "Сбой DNS", "warn"],
    ["upstream_degraded", "Проблема сети", "warn"],
    ["target_unavailable", "Сервис недоступен", "warn"],
    ["service_slow", "Сервис отвечает медленно", "warn"],
    ["sensor_unreliable", "Данные ненадёжны", "danger"],
  ])("maps %s lane assessment to %s", (classification, label, tone) => {
    expect(
      getLegacyLaneDisplayModel(lane({ classification })),
    ).toEqual({ label, tone });
  });

  it("distinguishes a pending environment gate from passive evidence collection", () => {
    expect(
      getLegacyLaneDisplayModel(
        lane({ phase: "gate_pending", classification: "awaiting_evidence" }),
      ),
    ).toEqual({ label: "Проверка среды", tone: "warn" });
  });

  it("shows a recent Working confirmation instead of returning to data collection", () => {
    const recent = lane({ workingConfirmedRecently: true });
    expect(getLegacyLaneDisplayModel(recent)).toEqual({
      label: "Работало недавно",
      tone: "ok",
    });
    expect(getLegacyEvidenceLabel(recent)).toBe("недавнее подтверждение");

    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView status={status({ lanes: [recent] })} />,
    );
    expect(markup).toContain("Работало недавно");
    expect(markup).not.toContain("Сбор данных");
  });

  it("summarizes confidence and bounded evidence without target details", () => {
    expect(
      getLegacyEvidenceLabel(
        lane({
          confidence: "high",
          evidence: {
            workingFlows: 1,
            workingTargets: 1,
            resetFlows: 3,
            resetTargets: 2,
            blackholeFlows: 2,
            blackholeTargets: 2,
          },
        }),
      ),
    ).toBe(
      "уверенность: высокая · успехи 1/1 · сбросы ≥3/≥2 · таймауты ≥2/≥2",
    );
  });

  it("renders a compact lane journal and an explicitly unexecuted switch intent", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          lanes: [
            lane({
              phase: "suspect",
              classification: "dpi_suspected",
              confidence: "high",
              evidence: {
                ...EMPTY_EVIDENCE,
                resetFlows: 3,
                resetTargets: 2,
              },
            }),
          ],
          presumedIntent: {
            kind: "switch_lane",
            category: "youtube_twitch",
            candidateConfig: "youtube_twitch_2.conf",
            reason: "dpi_suspected",
          },
        })}
      />,
    );

    expect(markup).toContain("Контроль надёжности");
    expect(markup).toContain("Только наблюдение");
    expect(markup).toContain("Оценка категорий");
    expect(markup).toContain("YouTube / Twitch");
    expect(markup).toContain("youtube_twitch_1.conf");
    expect(markup).toContain("Вероятна блокировка");
    expect(markup).toContain("сбросы ≥3/≥2");
    expect(markup).toContain("Сменить конфигурацию · YouTube / Twitch");
    expect(markup).toContain("Кандидат: youtube_twitch_2.conf");
    expect(markup).toContain(
      "Режим наблюдения: изменение не выполнено.",
    );
    expect(markup).not.toContain("Авто-восстановление");
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain('role="switch"');
  });

  it("marks a freeze intent as unexecuted and explains an ordinary wait", () => {
    const freezeMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          presumedIntent: {
            kind: "freeze_lane",
            category: "youtube_twitch",
            untilMs: 301_000,
            reason: "dpi_blocked",
          },
        })}
      />,
    );
    expect(freezeMarkup).toContain(
      "Приостановить восстановление · YouTube / Twitch",
    );
    expect(freezeMarkup).toContain(
      "Режим наблюдения: изменение не выполнено.",
    );

    const waitMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          presumedIntent: { kind: "wait", reason: "working" },
        })}
      />,
    );
    expect(waitMarkup).toContain("Изменение не требуется");
  });

  it("suppresses a stale action whenever observation is blind", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          phase: "blind",
          presumedIntent: {
            kind: "switch_lane",
            category: "youtube_twitch",
            candidateConfig: "youtube_twitch_2.conf",
            reason: "dpi_suspected",
          },
        })}
      />,
    );

    expect(markup).toContain("Наблюдение недоступно");
    expect(markup).toContain("Дождаться надёжных данных");
    expect(markup).not.toContain("Сменить конфигурацию");
    expect(markup).not.toContain("youtube_twitch_2.conf");
  });

  it("keeps the inactive capability read-only and hides session details", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          phase: "inactive",
          activeCategories: [],
          sessionId: null,
          sensorGeneration: null,
          lanes: [],
        })}
      />,
    );

    expect(markup).toContain("Ожидание запуска");
    expect(markup).toContain("Сбои фиксируются, конфигурации не меняются.");
    expect(markup).not.toContain("Оценка категорий");
    expect(markup).not.toContain("Предполагаемое действие");
    expect(markup).not.toContain("<button");
  });
});
