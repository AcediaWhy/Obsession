import { useState } from "react";

import type {
  LegacyReliabilityClassification,
  LegacyReliabilityConfidence,
  LegacyReliabilityLaneAssessment,
  LegacyReliabilityMode,
  LegacyReliabilityPhase,
  LegacyReliabilityProposal,
  LegacyReliabilityPresumedIntent,
  LegacyReliabilityStatus,
  LegacyRecoveryAttempt,
  LegacyRecoveryCompletion,
  LegacyRecoveryPhase,
} from "../../lib/tauri";
import { useLegacyReliabilityStore } from "../../store/legacyReliabilityStore";
import { useSettingsStore } from "../../store/settingsStore";
import { Button, SectionLabel, Switch } from "./atoms";

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

const RECOVERY_PHASE_DISPLAY = {
  preflight: {
    label: "Проверка безопасности",
    detail: "Шаг 1 из 4 · файлы, процесс и сеть",
    tone: "warn",
  },
  stopping: {
    label: "Остановка текущей конфигурации",
    detail: "Шаг 2 из 4 · только выбранная категория",
    tone: "warn",
  },
  starting: {
    label: "Запуск кандидата",
    detail: "Шаг 3 из 4 · ожидание готовности процесса",
    tone: "warn",
  },
  confirming: {
    label: "Проверка доступа",
    detail: "Шаг 4 из 4 · HTTPS и данные наблюдения",
    tone: "warn",
  },
  rolling_back: {
    label: "Возврат прежней конфигурации",
    detail: "Кандидат не подтверждён · выполняется безопасный откат",
    tone: "warn",
  },
  applied: {
    label: "Конфигурация применена",
    detail: "Проверка доступа завершена",
    tone: "ok",
  },
  process_failed: {
    label: "Требуется ручное вмешательство",
    detail: "Автоматическое восстановление остановлено",
    tone: "danger",
  },
} as const satisfies Record<
  LegacyRecoveryPhase,
  LegacyReliabilityDisplayModel & { detail: string }
>;

const COMPLETION_DISPLAY = {
  candidate_applied: {
    label: "Замена завершена",
    detail: "Новая конфигурация прошла проверку доступа.",
    tone: "ok",
  },
  previous_preserved: {
    label: "Прежняя конфигурация сохранена",
    detail: "Проверка остановлена до изменения процесса.",
    tone: "muted",
  },
  rolled_back: {
    label: "Выполнен откат",
    detail: "Кандидат не подошёл, прежняя конфигурация восстановлена.",
    tone: "warn",
  },
  process_failed: {
    label: "Нужно ручное вмешательство",
    detail: "Не удалось безопасно восстановить процесс. Перезапустите обход вручную.",
    tone: "danger",
  },
} as const satisfies Record<
  LegacyRecoveryCompletion["disposition"],
  LegacyReliabilityDisplayModel & { detail: string }
>;

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
    return { label: "Доступ подтверждён", tone: "ok" };
  }
  return CLASSIFICATION_DISPLAY[lane.classification];
}

export function getLegacyEvidenceLabel(
  lane: Pick<
    LegacyReliabilityLaneAssessment,
    "classification" | "confidence" | "workingConfirmedRecently"
  >,
): string {
  return lane.workingConfirmedRecently &&
    lane.classification === "awaiting_evidence"
    ? "подтверждено в текущей сессии"
    : CONFIDENCE_LABEL[lane.confidence];
}

function categoryLabel(category: string): string {
  return CATEGORY_LABEL[category] ?? category;
}

function categoryRank(category: string): number {
  const index = CATEGORY_ORDER.indexOf(category);
  return index === -1 ? CATEGORY_ORDER.length : index;
}

export function updateLegacyFrozenCategories(
  categories: readonly string[],
  category: string,
  frozen: boolean,
): string[] {
  const next = new Set(categories);
  if (frozen) next.add(category);
  else next.delete(category);
  return [...next].sort(
    (left, right) =>
      categoryRank(left) - categoryRank(right) || left.localeCompare(right),
  );
}

function pacingLabel(remainingMs: number): string {
  const seconds = Math.max(1, Math.ceil(remainingMs / 1_000));
  if (seconds < 60) return `около ${seconds} с`;
  const minutes = Math.ceil(seconds / 60);
  return `около ${minutes} мин`;
}

const MODE_LABEL: Record<LegacyReliabilityMode, string> = {
  observe_only: "Только наблюдение",
  assisted: "С подтверждением",
  automatic: "Автоматический режим",
};

function compactCategories(status: LegacyReliabilityStatus): string {
  const categories = status.activeCategories.length
    ? status.activeCategories
    : status.lanes.map((lane) => lane.category);
  return [...new Set(categories)]
    .sort((left, right) => categoryRank(left) - categoryRank(right))
    .map(categoryLabel)
    .join(" и ");
}

function compactStatus(
  status: LegacyReliabilityStatus,
): LegacyReliabilityDisplayModel {
  if (status.activeAttempt) {
    return RECOVERY_PHASE_DISPLAY[status.activeAttempt.phase];
  }
  if (status.phase !== "observing") {
    return getLegacyReliabilityDisplayModel(status.phase);
  }
  const lanes = status.lanes.map(getLegacyLaneDisplayModel);
  if (lanes.some((lane) => lane.tone === "danger")) {
    return { label: "Есть проблема", tone: "danger" };
  }
  if (lanes.some((lane) => lane.tone === "warn")) {
    return { label: "Требуется внимание", tone: "warn" };
  }
  if (lanes.length > 0 && lanes.every((lane) => lane.tone === "ok")) {
    return { label: "Всё работает", tone: "ok" };
  }
  return { label: "Наблюдение активно", tone: "ok" };
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
  mode,
}: {
  intent: LegacyReliabilityPresumedIntent;
  mode: LegacyReliabilityMode;
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
        {mode === "observe_only"
          ? "Режим наблюдения: изменение не выполнено."
          : mode === "assisted"
            ? "Без вашего подтверждения изменение не будет выполнено."
            : intent.kind === "freeze_lane"
              ? "Автовосстановление категории приостановлено правилами безопасности."
              : "Перед автоматической заменой защитные проверки будут повторены."}
      </div>
    </div>
  );
}

function LegacyLaneRow({
  lane,
  automatic,
  frozen,
  configuredFrozen,
  halted,
  controlsPending,
  onFreezeChange,
}: {
  lane: LegacyReliabilityLaneAssessment;
  automatic: boolean;
  frozen: boolean;
  configuredFrozen: boolean;
  halted: boolean;
  controlsPending: boolean;
  onFreezeChange?: (category: string, frozen: boolean) => void;
}) {
  const display = getLegacyLaneDisplayModel(lane);
  const assessment = `${
    lane.phase === "blocked_cooldown" ? "пауза · " : ""
  }${getLegacyEvidenceLabel(lane)}`;
  const freezeSynchronized = frozen === configuredFrozen;
  const freezePending = controlsPending || !freezeSynchronized;

  return (
    <li className="border-t border-white/10 py-2 first:border-t-0">
      <div className="flex min-w-0 items-center gap-2.5">
        <span
          aria-hidden="true"
          className={`h-1.5 w-1.5 shrink-0 rounded-full ${
            display.tone === "ok"
              ? "bg-ok"
              : display.tone === "warn"
                ? "bg-warn"
                : display.tone === "danger"
                  ? "bg-danger"
                  : "bg-ink-muted"
          }`}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="truncate font-medium text-ink-soft">
              {categoryLabel(lane.category)}
            </span>
            <span
              className={`shrink-0 text-right font-semibold ${TONE_CLASS[display.tone]}`}
            >
              {display.label}
            </span>
          </div>
          <div className="mt-0.5 flex min-w-0 items-center justify-between gap-2 text-3xs text-ink-muted">
            <span
              className="min-w-0 flex-1 truncate"
              title={lane.activeConfig ?? undefined}
            >
              {lane.activeConfig ?? "Конфигурация не определена"}
            </span>
            <span className="shrink-0" title={assessment}>
              {assessment}
            </span>
          </div>
        </div>
        {automatic ? (
          halted ? (
            <span className="shrink-0 text-3xs font-semibold text-danger">
              Остановлено
            </span>
          ) : (
            <button
              type="button"
              aria-label={`${configuredFrozen ? "Разрешить" : "Приостановить"} автоматическую замену для ${categoryLabel(lane.category)}`}
              title={
                configuredFrozen
                  ? "Разрешить автоматическую замену"
                  : "Приостановить автоматическую замену"
              }
              disabled={freezePending || !onFreezeChange}
              onClick={() => onFreezeChange?.(lane.category, !configuredFrozen)}
              className={`no-drag shrink-0 rounded-lg border px-2 py-1 text-3xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-not-allowed disabled:opacity-50 ${
                frozen
                  ? "border-warn/30 bg-warn/10 text-warn"
                  : "border-glass-border bg-white/5 text-ink-muted hover:bg-white/10 hover:text-ink-soft"
              }`}
            >
              {freezePending
                ? "…"
                : configuredFrozen
                  ? "Включить"
                  : "Пауза"}
            </button>
          )
        ) : null}
      </div>
    </li>
  );
}

function LegacyModeSelector({
  mode,
  disabled,
  onChange,
}: {
  mode: LegacyReliabilityMode;
  disabled: boolean;
  onChange?: (mode: LegacyReliabilityMode) => void;
}) {
  const options: Array<{ value: LegacyReliabilityMode; label: string }> = [
    { value: "observe_only", label: "Наблюдение" },
    { value: "assisted", label: "С подтверждением" },
    { value: "automatic", label: "Автоматически" },
  ];

  return (
    <div
      role="radiogroup"
      aria-label="Режим восстановления Legacy"
      className="grid grid-cols-3 gap-1 rounded-lg bg-black/10 p-1"
    >
      {options.map((option) => {
        const selected = mode === option.value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            title={option.label}
            disabled={disabled || !onChange}
            onClick={() => {
              if (!selected) onChange?.(option.value);
            }}
            className={`no-drag min-w-0 rounded-md px-1.5 py-1.5 text-3xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-not-allowed disabled:opacity-50 ${
              selected
                ? "bg-accent/20 text-ink shadow-glow"
                : "text-ink-muted hover:bg-white/5 hover:text-ink-soft"
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

function LegacyAutomaticOptInCard({
  disabled,
  onConfirm,
  onCancel,
}: {
  disabled: boolean;
  onConfirm?: () => void;
  onCancel?: () => void;
}) {
  return (
    <div
      role="alert"
      className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2.5 text-xs"
    >
      <div className="font-semibold text-warn">
        Включить автоматическую замену?
      </div>
      <p className="mt-1 text-2xs leading-relaxed text-ink-soft">
        Приложение сможет само заменить конфигурацию только проблемной категории
        после проверки сети. Если кандидат не подойдёт, прежняя конфигурация
        будет восстановлена.
      </p>
      <div className="mt-2 grid grid-cols-2 gap-2">
        <Button
          className="py-1.5 text-2xs"
          disabled={disabled || !onConfirm}
          onClick={onConfirm}
        >
          Включить
        </Button>
        <Button
          variant="ghost"
          className="py-1.5 text-2xs"
          disabled={disabled || !onCancel}
          onClick={onCancel}
        >
          Отмена
        </Button>
      </div>
    </div>
  );
}

function isProposalStale(
  status: LegacyReliabilityStatus,
  proposal: LegacyReliabilityProposal,
): boolean {
  const lane = status.lanes.find(
    (candidate) => candidate.category === proposal.category,
  );
  return (
    status.phase === "inactive" ||
    status.phase === "starting" ||
    !status.activeCategories.includes(proposal.category) ||
    !lane ||
    lane.activeConfig !== proposal.previousConfigId ||
    status.lastCompletion?.attemptId === proposal.attemptId
  );
}

function proposalUnavailableReason(
  status: LegacyReliabilityStatus,
  configuredMode: LegacyReliabilityMode,
  proposal: LegacyReliabilityProposal,
): string | null {
  if (configuredMode !== "assisted" || status.mode !== "assisted") {
    return "Подтверждение недоступно в текущем режиме.";
  }
  if (status.phase === "blind") {
    return "Подтверждение недоступно: наблюдение не видит трафик.";
  }
  if (status.phase === "degraded") {
    return "Подтверждение недоступно: данные наблюдения ненадёжны.";
  }
  if (isProposalStale(status, proposal)) {
    return "Предложение устарело; дождитесь обновления.";
  }
  return null;
}

function LegacyProposalCard({
  status,
  configuredMode,
  proposal,
  approvalPending,
  approvalError,
  onApprove,
}: {
  status: LegacyReliabilityStatus;
  configuredMode: LegacyReliabilityMode;
  proposal: LegacyReliabilityProposal;
  approvalPending: boolean;
  approvalError: string;
  onApprove?: (proposal: LegacyReliabilityProposal) => void;
}) {
  const unavailable = proposalUnavailableReason(
    status,
    configuredMode,
    proposal,
  );
  const assistedActive =
    configuredMode === "assisted" && status.mode === "assisted";

  return (
    <div className="rounded-lg border border-warn/35 bg-warn/10 px-3 py-2.5 text-xs">
      <div className="flex items-center justify-between gap-3">
        <span className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
          Предложение на 30 секунд
        </span>
        <span className="font-semibold text-warn">
          {categoryLabel(proposal.category)}
        </span>
      </div>
      <div className="mt-2 flex min-w-0 items-center gap-2 font-mono text-2xs text-ink-soft">
        <span className="min-w-0 truncate" title={proposal.previousConfigId}>
          {proposal.previousConfigId}
        </span>
        <span aria-hidden="true" className="shrink-0 text-ink-muted">
          →
        </span>
        <span
          className="min-w-0 truncate font-semibold text-ink"
          title={proposal.candidateConfigId}
        >
          {proposal.candidateConfigId}
        </span>
      </div>
      <p className="mt-1.5 text-2xs leading-relaxed text-ink-muted">
        Актуальность и защитные проверки контролирует приложение.
      </p>

      {assistedActive ? (
        <div className="mt-2">
          <Button
            className="w-full py-2 text-xs"
            disabled={Boolean(unavailable) || approvalPending || !onApprove}
            onClick={() => onApprove?.(proposal)}
          >
            {approvalPending ? "Подтверждение…" : "Подтвердить замену"}
          </Button>
        </div>
      ) : null}

      {unavailable ? (
        <p className="mt-1.5 text-2xs text-warn">{unavailable}</p>
      ) : approvalPending ? (
        <p aria-live="polite" className="mt-1.5 text-2xs text-ink-muted">
          Повторно проверяем сеть и состояние процесса…
        </p>
      ) : null}

      {approvalError ? (
        <p role="alert" className="mt-1.5 break-words text-2xs text-danger">
          Не удалось подтвердить замену: {approvalError}
        </p>
      ) : null}
    </div>
  );
}

function LegacyActiveAttemptCard({
  attempt,
}: {
  attempt: LegacyRecoveryAttempt;
}) {
  const display = RECOVERY_PHASE_DISPLAY[attempt.phase];
  const automatic = attempt.origin.kind === "automatic";
  return (
    <div
      role={attempt.phase === "process_failed" ? "alert" : "status"}
      aria-live="polite"
      className={`rounded-lg border px-3 py-2.5 text-xs ${
        attempt.phase === "process_failed"
          ? "border-danger/35 bg-danger/10"
          : attempt.phase === "applied"
            ? "border-ok/35 bg-ok/10"
            : "border-accent/30 bg-accent/10"
      }`}
    >
      <div className="flex items-center justify-between gap-3">
        <span className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
          {automatic ? "Автоматическое восстановление" : "Восстановление по подтверждению"} · {categoryLabel(attempt.category)}
        </span>
        <span className={`text-right font-semibold ${TONE_CLASS[display.tone]}`}>
          {display.label}
        </span>
      </div>
      <div className="mt-1.5 truncate font-mono text-2xs text-ink-soft">
        {attempt.previousConfigId} → {attempt.candidateConfigId}
      </div>
      <p className="mt-1 text-2xs text-ink-muted">{display.detail}</p>
    </div>
  );
}

function LegacyCompletionCard({
  completion,
  activeConfig,
}: {
  completion: LegacyRecoveryCompletion;
  activeConfig: string | null;
}) {
  const display = COMPLETION_DISPLAY[completion.disposition];
  const automatic = completion.origin.kind === "automatic";

  return (
    <div
      role={completion.disposition === "process_failed" ? "alert" : "status"}
      className={`rounded-lg border px-3 py-2.5 text-xs ${
        completion.disposition === "candidate_applied"
          ? "border-ok/35 bg-ok/10"
          : completion.disposition === "process_failed"
            ? "border-danger/35 bg-danger/10"
            : "border-warn/30 bg-warn/10"
      }`}
    >
      <div className="flex items-center justify-between gap-3">
        <span className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
          {automatic ? "Автоматический результат" : "Результат по подтверждению"} · {categoryLabel(completion.category)}
        </span>
        <span className={`text-right font-semibold ${TONE_CLASS[display.tone]}`}>
          {display.label}
        </span>
      </div>
      <p className="mt-1.5 text-2xs leading-relaxed text-ink-soft">
        {display.detail}
      </p>
      {activeConfig ? (
        <div className="mt-1 truncate font-mono text-3xs text-ink-muted">
          Активно: {activeConfig}
        </div>
      ) : null}
    </div>
  );
}

export function LegacyReliabilityPanelView({
  status,
  configuredEnabled = true,
  configuredMode = status.mode,
  configuredAutomaticPaused = status.automaticPaused,
  configuredFrozenCategories = status.frozenCategories,
  detailsExpanded = true,
  technicalDetailsExpanded = true,
  automaticOptInRequested = false,
  modeChangePending = false,
  approvalPending = false,
  approvalError = "",
  onEnabledChange,
  onDetailsExpandedChange,
  onTechnicalDetailsExpandedChange,
  onModeChange,
  onAutomaticOptInConfirm,
  onAutomaticOptInCancel,
  onAutomaticPauseChange,
  onLaneFreezeChange,
  onApprove,
}: {
  status: LegacyReliabilityStatus;
  configuredEnabled?: boolean;
  configuredMode?: LegacyReliabilityMode;
  configuredAutomaticPaused?: boolean;
  configuredFrozenCategories?: string[];
  detailsExpanded?: boolean;
  technicalDetailsExpanded?: boolean;
  automaticOptInRequested?: boolean;
  modeChangePending?: boolean;
  approvalPending?: boolean;
  approvalError?: string;
  onEnabledChange?: (enabled: boolean) => void;
  onDetailsExpandedChange?: (expanded: boolean) => void;
  onTechnicalDetailsExpandedChange?: (expanded: boolean) => void;
  onModeChange?: (mode: LegacyReliabilityMode) => void;
  onAutomaticOptInConfirm?: () => void;
  onAutomaticOptInCancel?: () => void;
  onAutomaticPauseChange?: (paused: boolean) => void;
  onLaneFreezeChange?: (category: string, frozen: boolean) => void;
  onApprove?: (proposal: LegacyReliabilityProposal) => void;
}) {
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
  const modeSynchronized = configuredMode === status.mode;
  const automaticActive = modeSynchronized && status.mode === "automatic";
  const pauseSynchronized =
    configuredAutomaticPaused === status.automaticPaused;
  const automaticControlsPending = modeChangePending || !pauseSynchronized;
  const frozenCategories = new Set(status.frozenCategories);
  const configuredFrozen = new Set(configuredFrozenCategories);
  const haltedCategories = new Set(status.haltedCategories);
  const summary = compactStatus(status);
  const shutdownPending = !configuredEnabled && Boolean(status.activeAttempt);
  const summaryTitle = shutdownPending
    ? "Контроль доступа выключается"
    : configuredEnabled
      ? MODE_LABEL[configuredMode]
      : "Контроль доступа выключен";
  const categories = compactCategories(status);
  const summaryDetail = shutdownPending
    ? "Текущая замена безопасно завершится, затем контроль отключится"
    : !configuredEnabled
      ? "Конфигурации остаются без изменений"
      : status.activeAttempt
        ? `${categoryLabel(status.activeAttempt.category)} · ${RECOVERY_PHASE_DISPLAY[status.activeAttempt.phase].label}`
        : status.phase === "inactive"
          ? "Запустится вместе с обходом"
          : categories
            ? `${categories} под наблюдением`
            : summary.label;
  const technicalDetailsAvailable =
    sessionActive ||
    Boolean(status.lastCompletion) ||
    status.negativeCooldownCount > 0;
  const completionActiveConfig = status.lastCompletion
    ? (status.lanes.find(
        (lane) => lane.category === status.lastCompletion?.category,
      )?.activeConfig ?? null)
    : null;

  return (
    <div className="w-full">
      <SectionLabel>Контроль доступа</SectionLabel>
      <div className="rounded-xl border border-glass-border bg-white/5 p-2.5">
        <div className="flex min-w-0 items-center gap-2.5">
          <span
            aria-hidden="true"
            className={`h-2 w-2 shrink-0 rounded-full transition-colors ${
              !configuredEnabled
                ? "bg-ink-muted"
                : summary.tone === "ok"
                  ? "bg-ok shadow-[0_0_9px_1px_rgba(52,211,153,0.45)]"
                  : summary.tone === "warn"
                    ? "bg-warn"
                    : summary.tone === "danger"
                      ? "bg-danger"
                      : "bg-ink-muted"
            }`}
          />
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 items-center gap-2 text-xs">
              <span className="truncate font-semibold text-ink-soft">
                {summaryTitle}
              </span>
              {configuredEnabled ? (
                <span
                  className={`shrink-0 text-3xs font-semibold ${TONE_CLASS[summary.tone]}`}
                >
                  {summary.label}
                </span>
              ) : null}
            </div>
            <p className="mt-0.5 truncate text-3xs text-ink-muted" title={summaryDetail}>
              {summaryDetail}
            </p>
          </div>
          <Switch
            checked={configuredEnabled}
            disabled={modeChangePending || !onEnabledChange}
            ariaLabel={
              configuredEnabled
                ? "Выключить контроль доступа"
                : "Включить контроль доступа"
            }
            onChange={onEnabledChange ?? (() => {})}
          />
          {configuredEnabled ? (
            <button
              type="button"
              aria-label={detailsExpanded ? "Свернуть настройки" : "Настроить контроль доступа"}
              aria-expanded={detailsExpanded}
              disabled={modeChangePending || !onDetailsExpandedChange}
              onClick={() => onDetailsExpandedChange?.(!detailsExpanded)}
              className="no-drag shrink-0 rounded-lg px-1.5 py-1 text-xs font-semibold text-ink-muted transition-colors hover:bg-white/5 hover:text-ink-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40"
            >
              <span aria-hidden="true">{detailsExpanded ? "⌃" : "⌄"}</span>
            </button>
          ) : null}
        </div>

        {configuredEnabled && detailsExpanded ? (
          <div className="mt-2.5 border-t border-white/10 pt-2.5">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-3xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
                Режим
              </span>
              {modeChangePending || !modeSynchronized ? (
                <span aria-live="polite" className="text-3xs text-ink-muted">
                  Применяется…
                </span>
              ) : null}
            </div>
            <LegacyModeSelector
              mode={configuredMode}
              disabled={modeChangePending || automaticOptInRequested}
              onChange={onModeChange}
            />

            {automaticOptInRequested ? (
              <div className="mt-2">
                <LegacyAutomaticOptInCard
                  disabled={modeChangePending}
                  onConfirm={onAutomaticOptInConfirm}
                  onCancel={onAutomaticOptInCancel}
                />
              </div>
            ) : null}

            {automaticActive ? (
              <div className="mt-2 flex items-center gap-3 rounded-xl bg-white/5 px-3 py-2">
                <div className="min-w-0 flex-1">
                  <div className="text-xs font-medium text-ink-soft">Автозамена</div>
                  <div className="mt-0.5 truncate text-3xs text-ink-muted">
                    {status.automaticPaused
                      ? "Новые попытки приостановлены"
                      : status.automaticPacingRemainingMs !== null &&
                          status.automaticPacingRemainingMs > 0
                        ? `Следующая попытка: ${pacingLabel(status.automaticPacingRemainingMs)}`
                        : "Включена для проблемных категорий"}
                  </div>
                </div>
                <Switch
                  checked={!configuredAutomaticPaused}
                  disabled={automaticControlsPending || !onAutomaticPauseChange}
                  ariaLabel={
                    configuredAutomaticPaused
                      ? "Возобновить автоматическую замену"
                      : "Приостановить автоматическую замену"
                  }
                  onChange={(active) => onAutomaticPauseChange?.(!active)}
                />
              </div>
            ) : null}

            {lanes.length > 0 ? (
              <ul
                aria-label="Состояние категорий"
                className="mt-2 rounded-xl bg-white/5 px-3"
              >
                {lanes.map((lane) => (
                  <LegacyLaneRow
                    key={`${lane.category}:${lane.laneGeneration}`}
                    lane={lane}
                    automatic={automaticActive}
                    frozen={frozenCategories.has(lane.category)}
                    configuredFrozen={configuredFrozen.has(lane.category)}
                    halted={haltedCategories.has(lane.category)}
                    controlsPending={modeChangePending}
                    onFreezeChange={onLaneFreezeChange}
                  />
                ))}
              </ul>
            ) : null}

            {status.proposal ? (
              <div className="mt-2">
                <LegacyProposalCard
                  status={status}
                  configuredMode={configuredMode}
                  proposal={status.proposal}
                  approvalPending={approvalPending}
                  approvalError={approvalError}
                  onApprove={onApprove}
                />
              </div>
            ) : null}

            {status.activeAttempt ? (
              <div className="mt-2">
                <LegacyActiveAttemptCard attempt={status.activeAttempt} />
              </div>
            ) : null}

            {technicalDetailsAvailable ? (
              <button
                type="button"
                aria-expanded={technicalDetailsExpanded}
                disabled={!onTechnicalDetailsExpandedChange}
                onClick={() =>
                  onTechnicalDetailsExpandedChange?.(!technicalDetailsExpanded)
                }
                className="no-drag mt-2 w-full rounded-lg px-2 py-1.5 text-center text-3xs font-semibold text-ink-muted transition-colors hover:bg-white/5 hover:text-ink-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40"
              >
                Технические сведения {technicalDetailsExpanded ? "⌃" : "⌄"}
              </button>
            ) : null}

            {technicalDetailsExpanded ? (
              <div className="mt-2 border-t border-white/10 pt-2">
                {!status.activeAttempt && !status.proposal && sessionActive ? (
                  <LegacyIntentCallout intent={safeIntent} mode={status.mode} />
                ) : null}
                {status.lastCompletion ? (
                  <div className="mt-2 first:mt-0">
                    <div className="mb-1.5 px-1 text-3xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
                      Последний результат
                    </div>
                    <LegacyCompletionCard
                      completion={status.lastCompletion}
                      activeConfig={completionActiveConfig}
                    />
                  </div>
                ) : null}
                {status.negativeCooldownCount > 0 ? (
                  <p className="mt-2 px-1 text-3xs text-ink-muted">
                    Кандидаты после неудачной проверки: {status.negativeCooldownCount}
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

export function LegacyReliabilityPanel() {
  const [automaticOptInRequested, setAutomaticOptInRequested] = useState(false);
  const [detailsExpanded, setDetailsExpanded] = useState(false);
  const [technicalDetailsExpanded, setTechnicalDetailsExpanded] = useState(false);
  const status = useLegacyReliabilityStore((state) => state.status);
  const approvalPending = useLegacyReliabilityStore(
    (state) => state.approvalPending,
  );
  const approvalError = useLegacyReliabilityStore(
    (state) => state.approvalError,
  );
  const approveProposal = useLegacyReliabilityStore(
    (state) => state.approveProposal,
  );
  const clearApprovalError = useLegacyReliabilityStore(
    (state) => state.clearApprovalError,
  );
  const configuredMode = useSettingsStore(
    (state) => state.settings?.legacy_reliability_mode,
  );
  const configuredEnabled = useSettingsStore(
    (state) => state.settings?.legacy_reliability_enabled,
  );
  const configuredAutomaticPaused = useSettingsStore(
    (state) => state.settings?.legacy_automatic_paused,
  );
  const configuredFrozenCategories = useSettingsStore(
    (state) => state.settings?.legacy_reliability_frozen_categories,
  );
  const settingsSaving = useSettingsStore((state) => state.saving);
  const patchSettings = useSettingsStore((state) => state.patch);

  return (
    <LegacyReliabilityPanelView
      status={status}
      configuredEnabled={configuredEnabled ?? true}
      configuredMode={configuredMode ?? status.mode}
      configuredAutomaticPaused={
        configuredAutomaticPaused ?? status.automaticPaused
      }
      configuredFrozenCategories={
        configuredFrozenCategories ?? status.frozenCategories
      }
      automaticOptInRequested={
        automaticOptInRequested && configuredMode !== "automatic"
      }
      detailsExpanded={detailsExpanded}
      technicalDetailsExpanded={technicalDetailsExpanded}
      modeChangePending={settingsSaving}
      approvalPending={approvalPending}
      approvalError={approvalError}
      onEnabledChange={
        configuredEnabled !== undefined
          ? (enabled) => {
              clearApprovalError();
              setAutomaticOptInRequested(false);
              if (!enabled) {
                setDetailsExpanded(false);
                setTechnicalDetailsExpanded(false);
              }
              void patchSettings({ legacy_reliability_enabled: enabled });
            }
          : undefined
      }
      onDetailsExpandedChange={setDetailsExpanded}
      onTechnicalDetailsExpandedChange={setTechnicalDetailsExpanded}
      onModeChange={
        configuredMode
          ? (mode) => {
              clearApprovalError();
              if (mode === "automatic" && configuredMode !== "automatic") {
                setAutomaticOptInRequested(true);
                return;
              }
              setAutomaticOptInRequested(false);
              void patchSettings({ legacy_reliability_mode: mode });
            }
          : undefined
      }
      onAutomaticOptInConfirm={
        configuredMode
          ? () => {
              setAutomaticOptInRequested(false);
              clearApprovalError();
              void patchSettings({ legacy_reliability_mode: "automatic" });
            }
          : undefined
      }
      onAutomaticOptInCancel={() => setAutomaticOptInRequested(false)}
      onAutomaticPauseChange={
        configuredMode
          ? (paused) => {
              void patchSettings({ legacy_automatic_paused: paused });
            }
          : undefined
      }
      onLaneFreezeChange={
        configuredFrozenCategories
          ? (category, frozen) => {
              const categories =
                useSettingsStore.getState().settings
                  ?.legacy_reliability_frozen_categories ?? [];
              void patchSettings({
                legacy_reliability_frozen_categories:
                  updateLegacyFrozenCategories(categories, category, frozen),
              });
            }
          : undefined
      }
      onApprove={(proposal) => {
        void approveProposal(proposal);
      }}
    />
  );
}
