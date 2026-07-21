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
import { Button, SectionLabel } from "./atoms";

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
    "confidence" | "evidence" | "workingConfirmedRecently"
  >,
): string {
  const evidence = lane.evidence;
  const parts = [
    lane.workingConfirmedRecently && lane.confidence === "none"
      ? "подтверждено в текущей сессии"
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
          : "Без вашего подтверждения изменение не будет выполнено."}
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
    { value: "observe_only", label: "Только наблюдение" },
    { value: "assisted", label: "С подтверждением" },
  ];

  return (
    <div
      role="radiogroup"
      aria-label="Режим восстановления Legacy"
      className="grid grid-cols-2 gap-1 rounded-lg bg-black/10 p-1"
    >
      {options.map((option) => {
        const selected = mode === option.value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            disabled={disabled || !onChange}
            onClick={() => onChange?.(option.value)}
            className={`no-drag rounded-md px-2 py-1.5 text-2xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-not-allowed disabled:opacity-50 ${
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
          Восстановление · {categoryLabel(attempt.category)}
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
}: {
  completion: LegacyRecoveryCompletion;
}) {
  const display = COMPLETION_DISPLAY[completion.disposition];
  const activeConfig =
    completion.disposition === "candidate_applied"
      ? completion.candidateConfigId
      : completion.disposition === "process_failed"
        ? null
        : completion.previousConfigId;

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
          Результат · {categoryLabel(completion.category)}
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
  configuredMode = status.mode,
  modeChangePending = false,
  approvalPending = false,
  approvalError = "",
  onModeChange,
  onApprove,
}: {
  status: LegacyReliabilityStatus;
  configuredMode?: LegacyReliabilityMode;
  modeChangePending?: boolean;
  approvalPending?: boolean;
  approvalError?: string;
  onModeChange?: (mode: LegacyReliabilityMode) => void;
  onApprove?: (proposal: LegacyReliabilityProposal) => void;
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
  const modeSynchronized = configuredMode === status.mode;

  const activity = status.activeAttempt ? (
    <LegacyActiveAttemptCard attempt={status.activeAttempt} />
  ) : status.proposal ? (
    <LegacyProposalCard
      status={status}
      configuredMode={configuredMode}
      proposal={status.proposal}
      approvalPending={approvalPending}
      approvalError={approvalError}
      onApprove={onApprove}
    />
  ) : status.lastCompletion ? (
    <LegacyCompletionCard completion={status.lastCompletion} />
  ) : sessionActive ? (
    <LegacyIntentCallout intent={safeIntent} mode={status.mode} />
  ) : null;

  return (
    <div className="w-full">
      <SectionLabel>Контроль надёжности</SectionLabel>
      <div className="flex flex-col gap-2 rounded-lg bg-white/5 px-3 py-2.5 text-sm">
        <div>
          <div className="mb-1.5 flex items-center justify-between gap-3">
            <span className="text-xs text-ink-muted">Режим восстановления</span>
            {modeChangePending || !modeSynchronized ? (
              <span aria-live="polite" className="text-3xs text-ink-muted">
                Применяется…
              </span>
            ) : null}
          </div>
          <LegacyModeSelector
            mode={configuredMode}
            disabled={modeChangePending}
            onChange={onModeChange}
          />
        </div>
        <div className="flex items-center justify-between gap-3">
          <span className="text-xs text-ink-muted">Состояние наблюдения</span>
          <span
            className={`text-right text-xs font-semibold ${TONE_CLASS[display.tone]}`}
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

      {activity ? <div className="mt-2">{activity}</div> : null}

      {status.negativeCooldownCount > 0 ? (
        <p className="mt-1.5 px-1 text-3xs text-ink-muted">
          Кандидаты на паузе после неудачной проверки: {status.negativeCooldownCount}
        </p>
      ) : null}

      <p className="mt-2 px-1 text-2xs leading-relaxed text-ink-muted">
        {!modeSynchronized
          ? "Настройка режима применяется. До завершения конфигурации не меняются."
          : configuredMode === "assisted"
            ? "Замена выполняется только после вашего подтверждения."
            : "Сбои фиксируются, конфигурации не меняются."}
      </p>
    </div>
  );
}

export function LegacyReliabilityPanel() {
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
  const settingsSaving = useSettingsStore((state) => state.saving);
  const patchSettings = useSettingsStore((state) => state.patch);

  return (
    <LegacyReliabilityPanelView
      status={status}
      configuredMode={configuredMode ?? status.mode}
      modeChangePending={settingsSaving}
      approvalPending={approvalPending}
      approvalError={approvalError}
      onModeChange={
        configuredMode
          ? (mode) => {
              clearApprovalError();
              void patchSettings({ legacy_reliability_mode: mode });
            }
          : undefined
      }
      onApprove={(proposal) => {
        void approveProposal(proposal);
      }}
    />
  );
}
