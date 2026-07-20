import type {
  LegacyReliabilityClassification,
  LegacyReliabilityConfidence,
  LegacyReliabilityLaneAssessment,
  LegacyReliabilityPhase,
  LegacyReliabilityPresumedIntent,
  LegacyReliabilityStatus,
} from "../../lib/tauri";
import { useLegacyReliabilityStore } from "../../store/legacyReliabilityStore";
import { SectionLabel } from "./atoms";

export type LegacyReliabilityTone = "muted" | "ok" | "warn" | "danger";

export interface LegacyReliabilityDisplayModel {
  label: string;
  tone: LegacyReliabilityTone;
}

const PHASE_DISPLAY = {
  inactive: { label: "Ожидание запуска", tone: "muted" },
  starting: { label: "Запуск наблюдения", tone: "muted" },
  observing: { label: "Наблюдение", tone: "ok" },
  degraded: { label: "Наблюдение ограничено", tone: "warn" },
  blind: { label: "Наблюдение недоступно", tone: "danger" },
} as const satisfies Record<
  LegacyReliabilityPhase,
  LegacyReliabilityDisplayModel
>;

const CLASSIFICATION_DISPLAY = {
  awaiting_evidence: { label: "Сбор данных", tone: "muted" },
  working: { label: "Работает", tone: "ok" },
  dpi_suspected: { label: "Вероятна блокировка", tone: "warn" },
  dpi_blocked: { label: "Блокировка подтверждена", tone: "danger" },
  offline: { label: "Нет подключения", tone: "warn" },
  dns_failure: { label: "Сбой DNS", tone: "warn" },
  upstream_degraded: { label: "Проблема сети", tone: "warn" },
  target_unavailable: { label: "Сервис недоступен", tone: "warn" },
  service_slow: { label: "Сервис отвечает медленно", tone: "warn" },
  sensor_unreliable: { label: "Данные ненадёжны", tone: "danger" },
} as const satisfies Record<
  LegacyReliabilityClassification,
  LegacyReliabilityDisplayModel
>;

const CONFIDENCE_LABEL: Record<LegacyReliabilityConfidence, string> = {
  none: "оценка не сформирована",
  low: "уверенность: низкая",
  medium: "уверенность: средняя",
  high: "уверенность: высокая",
};

const CATEGORY_LABEL: Record<string, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube / Twitch",
  gaming: "Gaming",
  universal: "Universal",
  atrisk: "Под угрозой",
};

const CATEGORY_ORDER = [
  "discord",
  "youtube_twitch",
  "gaming",
  "universal",
  "atrisk",
];

const TONE_CLASS: Record<LegacyReliabilityTone, string> = {
  muted: "text-ink-muted",
  ok: "text-ok",
  warn: "text-warn",
  danger: "text-danger",
};

export function getLegacyReliabilityDisplayModel(
  phase: LegacyReliabilityPhase,
): LegacyReliabilityDisplayModel {
  return PHASE_DISPLAY[phase];
}

export function getLegacyLaneDisplayModel(
  lane: Pick<
    LegacyReliabilityLaneAssessment,
    "phase" | "classification" | "workingConfirmedRecently"
  >,
): LegacyReliabilityDisplayModel {
  if (
    lane.classification === "awaiting_evidence" &&
    lane.phase === "gate_pending"
  ) {
    return { label: "Проверка среды", tone: "warn" };
  }
  if (
    lane.classification === "awaiting_evidence" &&
    lane.workingConfirmedRecently
  ) {
    return { label: "Работало недавно", tone: "ok" };
  }
  return CLASSIFICATION_DISPLAY[lane.classification];
}

export function getLegacyEvidenceLabel(
  lane: Pick<
    LegacyReliabilityLaneAssessment,
    "confidence" | "evidence" | "workingConfirmedRecently"
  >,
): string {
  const evidence = lane.evidence;
  const parts = [
    lane.workingConfirmedRecently && evidence.workingFlows === 0
      ? "недавнее подтверждение"
      : CONFIDENCE_LABEL[lane.confidence],
  ];
  if (evidence.workingFlows > 0) {
    parts.push(
      `успехи ${boundedCount(evidence.workingFlows, 2)}/${boundedCount(evidence.workingTargets, 2)}`,
    );
  }
  if (evidence.resetFlows > 0) {
    parts.push(
      `сбросы ${boundedCount(evidence.resetFlows, 3)}/${boundedCount(evidence.resetTargets, 2)}`,
    );
  }
  if (evidence.blackholeFlows > 0) {
    parts.push(
      `таймауты ${boundedCount(evidence.blackholeFlows, 2)}/${boundedCount(evidence.blackholeTargets, 2)}`,
    );
  }
  return parts.join(" · ");
}

function boundedCount(value: number, cap: number): string {
  return value >= cap ? `≥${cap}` : String(value);
}

function categoryLabel(category: string): string {
  return CATEGORY_LABEL[category] ?? category;
}

function categoryRank(category: string): number {
  const index = CATEGORY_ORDER.indexOf(category);
  return index === -1 ? CATEGORY_ORDER.length : index;
}

function waitIntentLabel(reason: LegacyReliabilityClassification): string {
  switch (reason) {
    case "working":
      return "Изменение не требуется";
    case "sensor_unreliable":
      return "Дождаться надёжных данных";
    case "dpi_blocked":
      return "Дождаться повторной проверки";
    case "offline":
    case "dns_failure":
    case "upstream_degraded":
    case "target_unavailable":
    case "service_slow":
      return "Не менять конфигурацию";
    case "awaiting_evidence":
    case "dpi_suspected":
      return "Продолжить наблюдение";
  }
}

function LegacyIntentCallout({
  intent,
}: {
  intent: LegacyReliabilityPresumedIntent;
}) {
  if (intent.kind === "wait") {
    return (
      <div className="flex items-center justify-between gap-3 rounded-lg bg-white/5 px-3 py-2 text-xs">
        <span className="text-ink-muted">Предполагаемое действие</span>
        <span className="text-right font-medium text-ink-soft">
          {waitIntentLabel(intent.reason)}
        </span>
      </div>
    );
  }

  const action =
    intent.kind === "switch_lane"
      ? `Сменить конфигурацию · ${categoryLabel(intent.category)}`
      : `Приостановить восстановление · ${categoryLabel(intent.category)}`;

  return (
    <div
      className={`rounded-lg border px-3 py-2 text-xs ${
        intent.kind === "switch_lane"
          ? "border-warn/35 bg-warn/10"
          : "border-danger/35 bg-danger/10"
      }`}
    >
      <div className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
        Предполагаемое действие
      </div>
      <div
        className={`mt-1 font-semibold ${
          intent.kind === "switch_lane" ? "text-warn" : "text-danger"
        }`}
      >
        {action}
      </div>
      {intent.kind === "switch_lane" ? (
        <div className="mt-0.5 truncate text-2xs text-ink-muted">
          Кандидат: {intent.candidateConfig}
        </div>
      ) : null}
      <div className="mt-1 text-2xs text-ink-soft">
        Режим наблюдения: изменение не выполнено.
      </div>
    </div>
  );
}

function LegacyLaneRow({ lane }: { lane: LegacyReliabilityLaneAssessment }) {
  const display = getLegacyLaneDisplayModel(lane);
  const evidence = getLegacyEvidenceLabel(lane);

  return (
    <li className="rounded-lg bg-white/5 px-3 py-2">
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="font-medium text-ink-soft">
          {categoryLabel(lane.category)}
        </span>
        <span
          className={`text-right font-semibold ${TONE_CLASS[display.tone]}`}
        >
          {display.label}
        </span>
      </div>
      <div className="mt-1 flex items-center justify-between gap-3 text-3xs text-ink-muted">
        <span className="min-w-0 truncate" title={lane.activeConfig ?? undefined}>
          {lane.activeConfig ?? "Конфигурация не определена"}
        </span>
        <span className="shrink-0 text-right">
          {lane.phase === "blocked_cooldown" ? "пауза · " : ""}
          {evidence}
        </span>
      </div>
    </li>
  );
}

export function LegacyReliabilityPanelView({
  status,
}: {
  status: LegacyReliabilityStatus;
}) {
  const display = getLegacyReliabilityDisplayModel(status.phase);
  const lanes = [...status.lanes].sort(
    (left, right) =>
      categoryRank(left.category) - categoryRank(right.category) ||
      left.category.localeCompare(right.category),
  );
  const sessionActive = status.phase !== "inactive";
  const safeIntent: LegacyReliabilityPresumedIntent =
    status.phase === "blind"
      ? { kind: "wait", reason: "sensor_unreliable" }
      : status.presumedIntent;

  return (
    <div className="w-full">
      <SectionLabel>Контроль надёжности</SectionLabel>
      <div className="flex flex-col gap-1 rounded-lg bg-white/5 px-3 py-2 text-sm">
        <div className="flex items-center justify-between gap-3">
          <span className="text-ink-muted">Режим</span>
          <span className="font-semibold text-ink-soft">
            Только наблюдение
          </span>
        </div>
        <div className="flex items-center justify-between gap-3">
          <span className="text-ink-muted">Состояние</span>
          <span
            className={`text-right font-semibold ${TONE_CLASS[display.tone]}`}
          >
            {display.label}
          </span>
        </div>
      </div>

      {lanes.length > 0 ? (
        <div className="mt-3">
          <div className="mb-1.5 px-1 text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
            Оценка категорий
          </div>
          <ul aria-label="Оценки категорий" className="flex flex-col gap-1.5">
            {lanes.map((lane) => (
              <LegacyLaneRow
                key={`${lane.category}:${lane.laneGeneration}`}
                lane={lane}
              />
            ))}
          </ul>
        </div>
      ) : null}

      {sessionActive ? (
        <div className="mt-2">
          <LegacyIntentCallout intent={safeIntent} />
        </div>
      ) : null}

      <p className="mt-2 px-1 text-2xs leading-relaxed text-ink-muted">
        Сбои фиксируются, конфигурации не меняются.
      </p>
    </div>
  );
}

export function LegacyReliabilityPanel() {
  const status = useLegacyReliabilityStore((state) => state.status);
  return <LegacyReliabilityPanelView status={status} />;
}
