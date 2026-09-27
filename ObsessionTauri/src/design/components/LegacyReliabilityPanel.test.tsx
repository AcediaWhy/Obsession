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
  updateLegacyFrozenCategories,
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
    runningApplications: [],
    sessionId: 11,
    sensorGeneration: 3,
    lanes: [lane()],
    presumedIntent: {
      kind: "wait",
      reason: "awaiting_evidence",
    },
    proposal: null,
    activeAttempt: null,
    lastCompletion: null,
    negativeCooldownCount: 0,
    automaticPaused: false,
    automaticPacingRemainingMs: null,
    frozenCategories: [],
    haltedCategories: [],
    ...overrides,
  };
}

const PROPOSAL = {
  proposalId: 5,
  attemptId: 8,
  incidentId: 3,
  category: "youtube_twitch",
  previousConfigId: "youtube_twitch_1.conf",
  candidateConfigId: "youtube_twitch_2.conf",
  expiresAtMonotonicMs: 93_000,
};

describe("LegacyReliabilityPanel", () => {
  it("keeps the enabled panel compact until the user opens settings", () => {
    const compactMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          lanes: [
            lane({
              classification: "working",
              confidence: "high",
              workingConfirmedRecently: true,
            }),
          ],
        })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded={false}
        onEnabledChange={() => {}}
        onDetailsExpandedChange={() => {}}
      />,
    );

    expect(compactMarkup).toContain("Контроль доступа");
    expect(compactMarkup).toContain("Автоматический режим");
    expect(compactMarkup).toContain("Всё работает");
    expect(compactMarkup).toContain('role="switch"');
    expect(compactMarkup).toContain('aria-checked="true"');
    expect(compactMarkup).toContain('aria-expanded="false"');
    expect(compactMarkup).not.toContain('role="radiogroup"');
    expect(compactMarkup).not.toContain("Оценка категорий");
    expect(compactMarkup).not.toContain("Предполагаемое действие");

    const expandedMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          lanes: [
            lane({
              classification: "working",
              confidence: "high",
              workingConfirmedRecently: true,
            }),
          ],
        })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded
        onEnabledChange={() => {}}
        onDetailsExpandedChange={() => {}}
      />,
    );

    expect(expandedMarkup).toContain('aria-expanded="true"');
    expect(expandedMarkup).toContain('role="radiogroup"');
    expect(expandedMarkup).toContain("YouTube / Twitch");
    expect(expandedMarkup).not.toContain("Оценка категорий");
  });

  it("collapses a disabled capability to one quiet row", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          lanes: [lane({ classification: "working", confidence: "high" })],
        })}
        configuredEnabled={false}
        configuredMode="automatic"
        detailsExpanded
        onEnabledChange={() => {}}
        onDetailsExpandedChange={() => {}}
      />,
    );

    expect(markup).toContain("Контроль доступа выключен");
    expect(markup).toContain("Конфигурации остаются без изменений");
    expect(markup).toContain('role="switch"');
    expect(markup).toContain('aria-checked="false"');
    expect(markup).not.toContain('role="radiogroup"');
    expect(markup).not.toContain("YouTube / Twitch");
    expect(markup).not.toContain("Предполагаемое действие");
    expect(markup).not.toContain("Последний результат");
  });

  it("explains safe shutdown without reopening the full panel", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          activeAttempt: {
            ...PROPOSAL,
            origin: { kind: "automatic", controlGeneration: 9 },
            phase: "confirming",
            phaseStartedAtMonotonicMs: 71_000,
          },
        })}
        configuredEnabled={false}
        configuredMode="automatic"
        detailsExpanded
        onEnabledChange={() => {}}
        onDetailsExpandedChange={() => {}}
      />,
    );

    expect(markup).toContain("Контроль доступа выключается");
    expect(markup).toContain(
      "Текущая замена безопасно завершится, затем контроль отключится",
    );
    expect(markup).not.toContain("Шаг 4 из 4");
    expect(markup).not.toContain('role="radiogroup"');
  });

  it("shows historical diagnostics in the single expanded level", () => {
    const completion = {
      ...PROPOSAL,
      origin: { kind: "automatic" as const, controlGeneration: 9 },
      phase: "applied" as const,
      disposition: "candidate_applied" as const,
      finishedAtMonotonicMs: 74_000,
    };

    // Свёрнутая панель диагностику не показывает.
    const collapsed = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ mode: "automatic", lastCompletion: completion })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded={false}
        onDetailsExpandedChange={() => {}}
      />,
    );
    expect(collapsed).not.toContain("Последний результат");
    expect(collapsed).not.toContain("Замена завершена");

    // Раскрытая — сразу, без второго тоггла.
    const expanded = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ mode: "automatic", lastCompletion: completion })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded
        onDetailsExpandedChange={() => {}}
      />,
    );
    expect(expanded).not.toContain("Технические сведения");
    expect(expanded).toContain("Последний результат");
    expect(expanded).toContain("Замена завершена");

    // Нечего показать — блоков нет и в раскрытом виде.
    const nothingToShow = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          phase: "inactive",
          lastCompletion: null,
        })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded
        onDetailsExpandedChange={() => {}}
      />,
    );
    expect(nothingToShow).not.toContain("Последний результат");
    expect(nothingToShow).not.toContain("Предполагаемое действие");
  });

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
    ["offline", "Нет подключения", "muted"],
    ["dns_failure", "Сбой DNS", "muted"],
    ["upstream_degraded", "Проблема сети", "muted"],
    ["target_unavailable", "Доступ не подтверждён", "muted"],
    ["service_slow", "Сервис отвечает медленно", "muted"],
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

  it("keeps a session Working confirmation instead of returning to data collection", () => {
    const confirmed = lane({ workingConfirmedRecently: true });
    expect(getLegacyLaneDisplayModel(confirmed)).toEqual({
      label: "Доступ подтверждён",
      tone: "ok",
    });
    expect(getLegacyEvidenceLabel(confirmed)).toBe(
      "подтверждено в текущей сессии",
    );

    const partial = lane({
      workingConfirmedRecently: true,
      confidence: "none",
      evidence: {
        ...confirmed.evidence,
        workingFlows: 1,
        workingTargets: 1,
      },
    });
    expect(getLegacyEvidenceLabel(partial)).toBe(
      "подтверждено в текущей сессии",
    );

    const transientTimeouts = lane({
      phase: "suspect",
      workingConfirmedRecently: true,
      confidence: "low",
      evidence: {
        ...confirmed.evidence,
        blackholeFlows: 2,
        blackholeTargets: 1,
      },
    });
    expect(getLegacyLaneDisplayModel(transientTimeouts)).toEqual({
      label: "Доступ подтверждён",
      tone: "ok",
    });
    expect(getLegacyEvidenceLabel(transientTimeouts)).toBe(
      "подтверждено в текущей сессии",
    );

    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ lanes: [transientTimeouts] })}
      />,
    );
    expect(markup).toContain("Доступ подтверждён");
    expect(markup).toContain("подтверждено в текущей сессии");
    expect(markup).not.toContain("таймауты");
    expect(markup).not.toContain("успехи");
    expect(markup).not.toContain("сбросы");
    expect(markup).not.toContain("Сбор данных");
    expect(markup).not.toContain("оценка не сформирована");
    expect(markup).not.toContain("уверенность: низкая");
  });

  it("keeps a fresh Working quorum at high confidence", () => {
    const working = lane({
      phase: "healthy",
      classification: "working",
      confidence: "high",
      workingConfirmedRecently: true,
      evidence: {
        ...EMPTY_EVIDENCE,
        workingFlows: 2,
        workingTargets: 2,
      },
    });

    expect(getLegacyLaneDisplayModel(working)).toEqual({
      label: "Работает",
      tone: "ok",
    });
    expect(getLegacyEvidenceLabel(working)).toBe("уверенность: высокая");
  });

  it("keeps raw evidence counters out of the compact status label", () => {
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
    ).toBe("уверенность: высокая");
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

    expect(markup).toContain("Контроль доступа");
    expect(markup).toContain("Наблюдение");
    expect(markup).not.toContain("Оценка категорий");
    expect(markup).toContain("YouTube / Twitch");
    expect(markup).toContain("youtube_twitch_1.conf");
    expect(markup).toContain("Вероятна блокировка");
    expect(markup).not.toContain("сбросы");
    expect(markup).toContain("Сменить конфигурацию · YouTube / Twitch");
    expect(markup).toContain("Кандидат: youtube_twitch_2.conf");
    expect(markup).toContain(
      "Режим наблюдения: изменение не выполнено.",
    );
    expect(markup).not.toContain("Авто-восстановление");
    expect(markup).not.toContain("Подтвердить замену");
    expect(markup).toContain('role="switch"');
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
    expect(markup).toContain("Запустится вместе с обходом");
    expect(markup).not.toContain("Оценка категорий");
    expect(markup).not.toContain("Предполагаемое действие");
    expect(markup).not.toContain("Подтвердить замену");
  });

  it("does not expose a detected Discord process while the observer is inactive", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          phase: "inactive",
          activeCategories: [],
          runningApplications: ["discord"],
          sessionId: null,
          sensorGeneration: null,
          lanes: [],
        })}
      />,
    );

    expect(markup).not.toContain("Discord");
    expect(markup).toContain("Запустится вместе с обходом");
  });

  it("does not add a Discord application callout under an active observer", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          activeCategories: ["discord"],
          runningApplications: ["discord"],
          lanes: [lane({ category: "discord" })],
        })}
      />,
    );

    expect(markup).toContain("Discord");
    expect(markup).toContain("Discord под наблюдением");
    expect(markup).not.toContain("Discord запущен");
  });

  it("offers explicit observe-only, assisted and automatic mode choices", () => {
    const observeMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status()}
        configuredMode="observe_only"
        onModeChange={() => {}}
      />,
    );
    expect(observeMarkup).toContain('role="radiogroup"');
    expect(observeMarkup).toContain("Наблюдение");
    expect(observeMarkup).toContain("С подтверждением");
    expect(observeMarkup).toContain("Автоматически");
    expect(observeMarkup).toMatch(
      /role="radio" aria-checked="true"[^>]*>Наблюдение/,
    );
    expect(observeMarkup).toContain("Только наблюдение");

    const assistedMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ mode: "assisted" })}
        configuredMode="assisted"
        onModeChange={() => {}}
      />,
    );
    expect(assistedMarkup).toMatch(
      /role="radio" aria-checked="true"[^>]*>С подтверждением/,
    );
    expect(assistedMarkup).toContain("С подтверждением");
  });

  it("does not claim a mode change is in flight while the observer is inactive", () => {
    // Пока наблюдатель не подключён, служба проецирует inactive() с
    // ObserveOnly независимо от настройки, поэтому расхождение здесь — норма,
    // а не незавершающийся спиннер.
    const inactiveMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ phase: "inactive", mode: "observe_only" })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded
        onModeChange={() => {}}
      />,
    );
    expect(inactiveMarkup).toContain("Запустится вместе с обходом");
    expect(inactiveMarkup).not.toContain("Применяется…");

    // Наблюдатель активен и режимы расходятся — изменение действительно в полёте.
    const observingMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ phase: "observing", mode: "observe_only" })}
        configuredEnabled
        configuredMode="automatic"
        detailsExpanded
        onModeChange={() => {}}
      />,
    );
    expect(observingMarkup).toContain("Применяется…");

    // Идущая запись настроек показывается всегда, даже без сессии.
    const savingMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ phase: "inactive", mode: "observe_only" })}
        configuredEnabled
        configuredMode="observe_only"
        detailsExpanded
        modeChangePending
        onModeChange={() => {}}
      />,
    );
    expect(savingMarkup).toContain("Применяется…");
  });

  it("requires an explicit second step before Automatic is enabled", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status()}
        configuredMode="observe_only"
        automaticOptInRequested
        onModeChange={() => {}}
        onAutomaticOptInConfirm={() => {}}
        onAutomaticOptInCancel={() => {}}
      />,
    );

    expect(markup).toContain("Включить автоматическую замену?");
    expect(markup).toContain(
      "само заменить конфигурацию только проблемной категории",
    );
    expect(markup).toContain("прежняя конфигурация");
    expect(markup).toContain(">Включить</button>");
    expect(markup).toContain(">Отмена</button>");
    expect(markup).not.toContain("Автозамена включена");
  });

  it("shows the armed Automatic state and backend-provided pacing", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          automaticPacingRemainingMs: 28_500,
          presumedIntent: {
            kind: "switch_lane",
            category: "youtube_twitch",
            candidateConfig: "youtube_twitch_2.conf",
            reason: "dpi_suspected",
          },
        })}
        configuredMode="automatic"
        onModeChange={() => {}}
      />,
    );

    expect(markup).toContain("Автозамена");
    expect(markup).toContain("Следующая попытка: около 29 с");
    expect(markup).toContain(
      "Перед автоматической заменой защитные проверки будут повторены.",
    );
    expect(markup).not.toContain(
      "Без вашего подтверждения изменение не будет выполнено.",
    );
  });

  it("offers no separate automatic-pause control", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          activeAttempt: {
            ...PROPOSAL,
            origin: { kind: "automatic", controlGeneration: 9 },
            phase: "confirming",
            phaseStartedAtMonotonicMs: 71_000,
          },
        })}
        configuredMode="automatic"
      />,
    );

    // Режим уже выражает согласие на автозамену, поэтому второго переключателя
    // для неё нет: глобальная остановка — смена режима, точечная — «Пауза» у
    // категории.
    expect(markup).not.toContain('aria-label="Возобновить автоматическую замену"');
    expect(markup).not.toContain(
      'aria-label="Приостановить автоматическую замену"',
    );
    expect(markup).not.toContain("Новые попытки приостановлены");
    expect(markup).not.toContain("Включена для проблемных категорий");
    // Питание контроля доступа — единственный switch в панели.
    expect(markup.match(/role="switch"/g)).toHaveLength(1);
    expect(markup).toContain("Проверка доступа");
    expect(markup).not.toContain("Уже начатая операция будет безопасно завершена");
  });

  it("keeps an Automatic attempt transparent after the mode is downgraded", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "observe_only",
          activeAttempt: {
            ...PROPOSAL,
            origin: { kind: "automatic", controlGeneration: 9 },
            phase: "stopping",
            phaseStartedAtMonotonicMs: 71_000,
          },
        })}
        configuredMode="observe_only"
      />,
    );

    expect(markup).toContain("Автоматическое восстановление");
    expect(markup).toContain("Остановка текущей конфигурации");
    expect(markup).not.toContain("Сбои фиксируются, конфигурации не меняются.");
  });

  it("renders per-lane freeze on its own future-attempt control line", () => {
    const frozenMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          frozenCategories: ["youtube_twitch"],
        })}
        configuredMode="automatic"
        configuredFrozenCategories={["youtube_twitch"]}
        onLaneFreezeChange={() => {}}
      />,
    );
    expect(frozenMarkup).toContain(">Включить</button>");
    expect(frozenMarkup).toContain(
      'aria-label="Разрешить автоматическую замену для YouTube / Twitch"',
    );

    const enabledMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ mode: "automatic" })}
        configuredMode="automatic"
        configuredFrozenCategories={[]}
        onLaneFreezeChange={() => {}}
      />,
    );
    expect(enabledMarkup).toContain(">Пауза</button>");
  });

  it("renders a terminal lane halt separately from a user freeze", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          haltedCategories: ["youtube_twitch"],
        })}
        configuredMode="automatic"
        onLaneFreezeChange={() => {}}
      />,
    );

    expect(markup).toContain("Остановлено");
    expect(markup).not.toContain(">Пауза</button>");
  });

  it("normalizes the frozen category patch without duplicates", () => {
    expect(
      updateLegacyFrozenCategories(
        ["gaming", "youtube_twitch", "gaming"],
        "discord",
        true,
      ),
    ).toEqual(["discord", "youtube_twitch", "gaming"]);
    expect(
      updateLegacyFrozenCategories(
        ["discord", "youtube_twitch"],
        "discord",
        false,
      ),
    ).toEqual(["youtube_twitch"]);
  });

  it("renders a backend-owned 30-second assisted proposal", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ mode: "assisted", proposal: PROPOSAL })}
        configuredMode="assisted"
        onModeChange={() => {}}
        onApprove={() => {}}
      />,
    );

    expect(markup).toContain("Предложение на 30 секунд");
    expect(markup).toContain("YouTube / Twitch");
    expect(markup).toContain("youtube_twitch_1.conf");
    expect(markup).toContain("youtube_twitch_2.conf");
    expect(markup).toContain("Подтвердить замену");
    expect(markup).not.toContain("Предложение устарело");
  });

  it("disables blind and stale proposals and never approves in observe-only", () => {
    const blindMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ mode: "assisted", phase: "blind", proposal: PROPOSAL })}
        configuredMode="assisted"
        onModeChange={() => {}}
        onApprove={() => {}}
      />,
    );
    expect(blindMarkup).toContain(
      "Подтверждение недоступно: наблюдение не видит трафик.",
    );
    expect(blindMarkup).toMatch(/disabled=""[^>]*>Подтвердить замену/);

    const staleMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "assisted",
          proposal: { ...PROPOSAL, previousConfigId: "obsolete.conf" },
        })}
        configuredMode="assisted"
        onModeChange={() => {}}
        onApprove={() => {}}
      />,
    );
    expect(staleMarkup).toContain("Предложение устарело");
    expect(staleMarkup).toMatch(/disabled=""[^>]*>Подтвердить замену/);

    const observeMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ proposal: PROPOSAL })}
        configuredMode="observe_only"
        onModeChange={() => {}}
        onApprove={() => {}}
      />,
    );
    expect(observeMarkup).not.toContain("Подтвердить замену");
    expect(observeMarkup).toContain(
      "Подтверждение недоступно в текущем режиме.",
    );
  });

  it("shows understandable attempt progress and rollback progress", () => {
    const confirmingMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "assisted",
          activeAttempt: {
            ...PROPOSAL,
            origin: { kind: "assisted" },
            phase: "confirming",
            phaseStartedAtMonotonicMs: 71_000,
          },
        })}
        configuredMode="assisted"
      />,
    );
    expect(confirmingMarkup).toContain("Проверка доступа");
    expect(confirmingMarkup).toContain("Шаг 4 из 4");
    expect(confirmingMarkup).toContain("HTTPS и данные наблюдения");

    const rollbackMarkup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "assisted",
          activeAttempt: {
            ...PROPOSAL,
            origin: { kind: "assisted" },
            phase: "rolling_back",
            phaseStartedAtMonotonicMs: 72_000,
          },
        })}
        configuredMode="assisted"
      />,
    );
    expect(rollbackMarkup).toContain("Возврат прежней конфигурации");
    expect(rollbackMarkup).toContain("выполняется безопасный откат");
  });

  it.each([
    ["candidate_applied", "Замена завершена", "youtube_twitch_2.conf"],
    [
      "previous_preserved",
      "Прежняя конфигурация сохранена",
      "youtube_twitch_1.conf",
    ],
    ["rolled_back", "Выполнен откат", "youtube_twitch_1.conf"],
    ["process_failed", "Нужно ручное вмешательство", "Перезапустите обход вручную"],
  ] as const)(
    "renders a distinct %s completion state",
    (disposition, label, detail) => {
      const markup = renderToStaticMarkup(
        <LegacyReliabilityPanelView
          status={status({
            mode: "assisted",
            lanes: [
              lane({
                activeConfig:
                  disposition === "candidate_applied"
                    ? "youtube_twitch_2.conf"
                    : disposition === "process_failed"
                      ? null
                      : "youtube_twitch_1.conf",
              }),
            ],
            lastCompletion: {
              ...PROPOSAL,
              origin: { kind: "assisted" },
              phase: disposition === "candidate_applied" ? "applied" : "rolling_back",
              disposition,
              finishedAtMonotonicMs: 81_000,
            },
          })}
          configuredMode="assisted"
        />,
      );
      expect(markup).toContain(label);
      expect(markup).toContain(detail);
    },
  );

  it("keeps a fresh recovery intent separate from historical completion", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({
          mode: "automatic",
          lanes: [
            lane({
              activeConfig: "youtube_twitch_test_broken.conf",
              classification: "dpi_blocked",
              confidence: "high",
            }),
          ],
          presumedIntent: {
            kind: "switch_lane",
            category: "youtube_twitch",
            candidateConfig: "youtube_twitch_2.conf",
            reason: "dpi_blocked",
          },
          lastCompletion: {
            ...PROPOSAL,
            origin: { kind: "automatic", controlGeneration: 4 },
            phase: "applied",
            disposition: "previous_preserved",
            finishedAtMonotonicMs: 81_000,
          },
        })}
        configuredMode="automatic"
      />,
    );

    expect(markup).toContain("Кандидат: youtube_twitch_2.conf");
    expect(markup).toContain("Последний результат");
    expect(markup).toContain("Активно: youtube_twitch_test_broken.conf");
    expect(markup).not.toContain("Активно: youtube_twitch_1.conf");
  });

  it("reports candidates in negative cooldown without exposing timers", () => {
    const markup = renderToStaticMarkup(
      <LegacyReliabilityPanelView
        status={status({ negativeCooldownCount: 2 })}
      />,
    );
    expect(markup).toContain("Кандидаты после неудачной проверки: 2");
  });
});
