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
import { Button, Chip, SectionLabel, Switch } from "./atoms";
import { Collapse } from "./Collapse";
import { Icon } from "./icons";

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
  offline: { label: "Нет подключения", tone: "muted" },
  dns_failure: { label: "Сбой DNS", tone: "muted" },
  upstream_degraded: { label: "Проблема сети", tone: "muted" },
  target_unavailable: { label: "Сервис недоступен", tone: "muted" },
  service_slow: { label: "Сервис отвечает медленно", tone: "muted" },
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
      <div className="flex items-center justify-between gap-3">
        <span className="text-2xs text-ink-muted">Предполагаемое действие</span>
        <span className="text-right text-sm font-medium text-ink-soft">
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
      className={`rounded-lg border px-3 py-2.5 ${
        intent.kind === "switch_lane"
          ? "border-warn/35 bg-warn/10"
          : "border-danger/35 bg-danger/10"
      }`}
    >
      <div className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
        Предполагаемое действие
      </div>
      <div
        className={`mt-1 text-sm font-semibold ${
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
      <div className="mt-1.5 text-2xs leading-relaxed text-ink-soft">
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
    <li className="flex min-w-0 items-center gap-3 border-t border-white/[0.06] px-3 py-2.5 first:border-t-0">
      <span
        aria-hidden="true"
        className={`h-2 w-2 shrink-0 rounded-full ${
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
        <div className="flex items-center justify-between gap-2">
          {/* truncate здесь — страховка от произвольно длинной подписи рядом с
              shrink-0 соседом, а не способ вписать заведомо длинный текст. */}
          <span className="min-w-0 truncate text-sm font-medium text-ink">
            {categoryLabel(lane.category)}
          </span>
          <span
            className={`shrink-0 text-2xs font-semibold ${TONE_CLASS[display.tone]}`}
          >
            {display.label}
          </span>
        </div>
        <div className="mt-0.5 flex min-w-0 items-center justify-between gap-2 text-2xs text-ink-muted">
          <span
            className="min-w-0 flex-1 truncate"
            title={lane.activeConfig ?? undefined}
          >
            {lane.activeConfig ?? "Конфигурация не определена"}
          </span>
          <span className="shrink-0">{assessment}</span>
        </div>
      </div>
      {automatic ? (
        halted ? (
          <span className="shrink-0 text-2xs font-semibold text-danger">
            Остановлено
          </span>
        ) : (
          <button
            type="button"
            aria-label={`${configuredFrozen ? "Разрешить" : "Приостановить"} автоматическую замену для ${categoryLabel(lane.category)}`}
            disabled={freezePending || !onFreezeChange}
            onClick={() => onFreezeChange?.(lane.category, !configuredFrozen)}
            className={`no-drag shrink-0 rounded-lg border px-2.5 py-1 text-2xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-not-allowed disabled:opacity-50 ${
              frozen
                ? "border-warn/30 bg-warn/10 text-warn"
                : "border-glass-border bg-white/5 text-ink-muted hover:bg-white/10 hover:text-ink-soft"
            }`}
          >
            {freezePending ? "…" : configuredFrozen ? "Включить" : "Пауза"}
          </button>
        )
      ) : null}
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
      className="grid gap-2"
    >
      {options.map((option) => {
        const selected = mode === option.value;
        return (
          <Chip
            key={option.value}
            role="radio"
            ariaChecked={selected}
            label={option.label}
            active={selected}
            disabled={disabled || !onChange}
            onClick={() => {
              if (!selected) onChange?.(option.value);
            }}
            className="w-full text-left"
          />
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
      className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2.5"
    >
      <div className="text-sm font-semibold text-warn">
        Включить автоматическую замену?
      </div>
      <p className="mt-1 text-2xs leading-relaxed text-ink-soft">
        Приложение сможет само заменить конфигурацию только проблемной категории
        после проверки сети. Если кандидат не подойдёт, прежняя конфигурация
        будет восстановлена.
      </p>
      <div className="mt-2.5 grid grid-cols-2 gap-2">
        <Button
          className="py-1.5 text-xs"
          disabled={disabled || !onConfirm}
          onClick={onConfirm}
        >
          Включить
        </Button>
        <Button
          variant="ghost"
          className="py-1.5 text-xs"
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
    <div className="rounded-lg border border-warn/35 bg-warn/10 px-3 py-2.5">
      <div className="flex items-center justify-between gap-3">
        <span className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
          Предложение на 30 секунд
        </span>
        <span className="shrink-0 text-sm font-semibold text-warn">
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
      className={`rounded-lg border px-3 py-2.5 ${
        attempt.phase === "process_failed"
          ? "border-danger/35 bg-danger/10"
          : attempt.phase === "applied"
            ? "border-ok/35 bg-ok/10"
            : "border-accent/30 bg-accent/10"
      }`}
    >
      {/* Надзаголовок отдельной строкой: «Автоматическое восстановление ·
          YouTube / Twitch» рядом с тональной подписью не вмещалось в колонку. */}
      <div className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
        {automatic ? "Автоматическое восстановление" : "Восстановление по подтверждению"} · {categoryLabel(attempt.category)}
      </div>
      <div className={`mt-1 text-sm font-semibold ${TONE_CLASS[display.tone]}`}>
        {display.label}
      </div>
      <div className="mt-1 truncate font-mono text-2xs text-ink-soft">
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
      className={`rounded-lg border px-3 py-2.5 ${
        completion.disposition === "candidate_applied"
          ? "border-ok/35 bg-ok/10"
          : completion.disposition === "process_failed"
            ? "border-danger/35 bg-danger/10"
            : "border-warn/30 bg-warn/10"
      }`}
    >
      <div className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
        {automatic ? "Автоматический результат" : "Результат по подтверждению"} · {categoryLabel(completion.category)}
      </div>
      <div className={`mt-1 text-sm font-semibold ${TONE_CLASS[display.tone]}`}>
        {display.label}
      </div>
      <p className="mt-1 text-2xs leading-relaxed text-ink-soft">
        {display.detail}
      </p>
      {activeConfig ? (
        <div className="mt-1 truncate font-mono text-2xs text-ink-muted">
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
  configuredFrozenCategories = status.frozenCategories,
  detailsExpanded = true,
  automaticOptInRequested = false,
  modeChangePending = false,
  approvalPending = false,
  approvalError = "",
  onEnabledChange,
  onDetailsExpandedChange,
  onModeChange,
  onAutomaticOptInConfirm,
  onAutomaticOptInCancel,
  onLaneFreezeChange,
  onApprove,
}: {
  status: LegacyReliabilityStatus;
  configuredEnabled?: boolean;
  configuredMode?: LegacyReliabilityMode;
  configuredFrozenCategories?: string[];
  detailsExpanded?: boolean;
  automaticOptInRequested?: boolean;
  modeChangePending?: boolean;
  approvalPending?: boolean;
  approvalError?: string;
  onEnabledChange?: (enabled: boolean) => void;
  onDetailsExpandedChange?: (expanded: boolean) => void;
  onModeChange?: (mode: LegacyReliabilityMode) => void;
  onAutomaticOptInConfirm?: () => void;
  onAutomaticOptInCancel?: () => void;
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
  const completionActiveConfig = status.lastCompletion
    ? (status.lanes.find(
        (lane) => lane.category === status.lastCompletion?.category,
      )?.activeConfig ?? null)
    : null;

  return (
    <section className="w-full">
      {/* Сводный статус — рядом с SectionLabel, как в Diagnostics. Питание
          остаётся в строке состояния, рядом с тем, что включает: так ни ярлык
          секции, ни заголовок режима не борются за одну и ту же ширину и
          ничего не приходится обрезать. */}
      <div className="mb-2 flex items-center justify-between gap-3">
        <SectionLabel>
          <span className="whitespace-nowrap">Контроль доступа</span>
        </SectionLabel>
        {configuredEnabled ? (
          <span
            className={`shrink-0 whitespace-nowrap text-2xs font-semibold ${TONE_CLASS[summary.tone]}`}
          >
            {summary.label}
          </span>
        ) : null}
      </div>

      <div className="overflow-hidden rounded-lg border border-white/[0.08]">
        <div className="flex items-center gap-3 px-3 py-2.5">
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
            <div className="text-sm font-medium text-ink">{summaryTitle}</div>
            <p className="mt-0.5 text-2xs text-ink-muted">{summaryDetail}</p>
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
              className="no-drag btn-anim shrink-0 rounded-lg p-1.5 text-ink-muted hover:bg-white/5 hover:text-ink-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40"
            >
              <Icon.Chevron
                size={16}
                className={`transition-transform duration-[var(--motion-fast)] ${
                  detailsExpanded ? "rotate-180" : ""
                }`}
              />
            </button>
          ) : null}
        </div>

        <Collapse open={configuredEnabled && detailsExpanded}>
              <div className="border-t border-white/[0.06] bg-white/[0.025]">
                <div className="px-3 py-3">
                  <div className="mb-2 flex items-center justify-between gap-2">
                    <span className="text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
                      Режим
                    </span>
                    {/* Пока наблюдатель не подключён, служба не сообщает режим
                        вообще: проекция падает в inactive() с ObserveOnly
                        (protected_runtime.rs), поэтому расхождение с настройкой
                        здесь — не «изменение в полёте», а просто «применится при
                        запуске». Об этом уже говорит строка состояния, а
                        незавершающийся спиннер только врал. Реальную запись
                        настроек по-прежнему показывает modeChangePending. */}
                    {modeChangePending || (!modeSynchronized && sessionActive) ? (
                      <span aria-live="polite" className="text-2xs text-ink-muted">
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
                    <div className="mt-3">
                      <LegacyAutomaticOptInCard
                        disabled={modeChangePending}
                        onConfirm={onAutomaticOptInConfirm}
                        onCancel={onAutomaticOptInCancel}
                      />
                    </div>
                  ) : null}
                </div>

                {/* Отдельного тумблера автозамены нет: в режиме «Автоматически»
                    она включена по определению, а согласие уже собрано карточкой
                    опт-ина. Глобальная остановка — это смена режима, точечная —
                    кнопка «Пауза» у категории. Остаётся только то, чего иначе
                    нигде не видно: когда будет следующая попытка. */}
                {automaticActive &&
                status.automaticPacingRemainingMs !== null &&
                status.automaticPacingRemainingMs > 0 ? (
                  <div className="border-t border-white/[0.06] px-3 py-2 text-2xs text-ink-muted">
                    Автозамена ·{" "}
                    {`Следующая попытка: ${pacingLabel(status.automaticPacingRemainingMs)}`}
                  </div>
                ) : null}

                {lanes.length > 0 ? (
                  <ul
                    aria-label="Состояние категорий"
                    className="border-t border-white/[0.06]"
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
                  <div className="border-t border-white/[0.06] px-3 py-3">
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
                  <div className="border-t border-white/[0.06] px-3 py-3">
                    <LegacyActiveAttemptCard attempt={status.activeAttempt} />
                  </div>
                ) : null}

                {/* Диагностика раскрыта наравне с остальным: у каждого блока своё
                    условие доступности, поэтому «нечего показать → ничего не
                    показываем» сохраняется без второго тоггла. */}
                {!status.activeAttempt && !status.proposal && sessionActive ? (
                  <div className="border-t border-white/[0.06] px-3 py-2.5">
                    <LegacyIntentCallout intent={safeIntent} mode={status.mode} />
                  </div>
                ) : null}

                {status.lastCompletion ? (
                  <div className="border-t border-white/[0.06] px-3 py-3">
                    <div className="mb-1.5 text-2xs font-semibold uppercase tracking-[0.12em] text-ink-muted">
                      Последний результат
                    </div>
                    <LegacyCompletionCard
                      completion={status.lastCompletion}
                      activeConfig={completionActiveConfig}
                    />
                  </div>
                ) : null}

                {status.negativeCooldownCount > 0 ? (
                  <p className="border-t border-white/[0.06] px-3 py-2.5 text-2xs text-ink-muted">
                    Кандидаты после неудачной проверки: {status.negativeCooldownCount}
                  </p>
                ) : null}
              </div>
        </Collapse>
      </div>
    </section>
  );
}

export function LegacyReliabilityPanel() {
  const [automaticOptInRequested, setAutomaticOptInRequested] = useState(false);
  const [detailsExpanded, setDetailsExpanded] = useState(false);
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
      configuredFrozenCategories={
        configuredFrozenCategories ?? status.frozenCategories
      }
      automaticOptInRequested={
        automaticOptInRequested && configuredMode !== "automatic"
      }
      detailsExpanded={detailsExpanded}
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
              }
              void patchSettings({ legacy_reliability_enabled: enabled });
            }
          : undefined
      }
      onDetailsExpandedChange={setDetailsExpanded}
      onModeChange={
        configuredMode
          ? (mode) => {
              clearApprovalError();
              if (mode === "automatic" && configuredMode !== "automatic") {
                setAutomaticOptInRequested(true);
                return;
              }
              setAutomaticOptInRequested(false);
              // legacy_automatic_paused больше не имеет своего элемента
              // управления, поэтому гасим его при любой смене режима: иначе
              // значение true из старого конфига навсегда осталось бы
              // включённым без способа его снять.
              void patchSettings({
                legacy_reliability_mode: mode,
                legacy_automatic_paused: false,
              });
            }
          : undefined
      }
      onAutomaticOptInConfirm={
        configuredMode
          ? () => {
              setAutomaticOptInRequested(false);
              clearApprovalError();
              void patchSettings({
                legacy_reliability_mode: "automatic",
                legacy_automatic_paused: false,
              });
            }
          : undefined
      }
      onAutomaticOptInCancel={() => setAutomaticOptInRequested(false)}
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
