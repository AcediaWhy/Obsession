import { useEffect, useState } from "react";

import { useAdaptiveStrategyStore } from "../../store/adaptiveStrategyStore";
import { useDpiStore } from "../../store/dpiStore";
import { useSettingsStore } from "../../store/settingsStore";
import type { AdaptiveCategory, AdaptivePhase } from "../../lib/tauri";
import { Button, Chip, SectionLabel, Switch } from "./atoms";

const CATEGORY_LABEL: Record<AdaptiveCategory, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube",
  gaming: "Gaming + GitHub",
};

const SEARCH_MODES = [
  { key: "fast", label: "Быстрый" },
  { key: "balanced", label: "Баланс" },
  { key: "deep", label: "Глубокий" },
] as const;

const ACTIVE_PHASES = new Set<AdaptivePhase>([
  "discovering_quic",
  "calibrating",
  "searching",
  "candidate_probe",
  "rolling_back",
  "applying",
]);

export function AdaptiveStrategyPanel() {
  const enabled = useSettingsStore(
    (state) => state.settings?.adaptive_strategy_enabled ?? false,
  );
  const patchSettings = useSettingsStore((state) => state.patch);
  const searchMode = useSettingsStore(
    (state) => state.settings?.adaptive_search_mode ?? "balanced",
  );
  const dpiActive = useDpiStore((state) => state.active);
  const selectedCategories = useDpiStore((state) => state.selectedCategories);
  const status = useAdaptiveStrategyStore((state) => state.status);
  const suggestion = useAdaptiveStrategyStore((state) => state.suggestion);
  const probe = useAdaptiveStrategyStore((state) => state.probe);
  const busy = useAdaptiveStrategyStore((state) => state.busy);
  const error = useAdaptiveStrategyStore((state) => state.error);
  const verificationEndsAt = useAdaptiveStrategyStore(
    (state) => state.verificationEndsAt,
  );
  const savedCandidateId = useAdaptiveStrategyStore(
    (state) => state.savedCandidateId,
  );
  const startSearch = useAdaptiveStrategyStore((state) => state.startSearch);
  const cancel = useAdaptiveStrategyStore((state) => state.cancel);
  const confirm = useAdaptiveStrategyStore((state) => state.confirm);
  const reject = useAdaptiveStrategyStore((state) => state.reject);
  const resetSaved = useAdaptiveStrategyStore((state) => state.resetSaved);
  const [remaining, setRemaining] = useState(60);
  const [searchTransport, setSearchTransport] = useState<"tls" | "quic">("tls");

  useEffect(() => {
    if (!verificationEndsAt) return;
    const update = () =>
      setRemaining(Math.max(0, Math.ceil((verificationEndsAt - Date.now()) / 1000)));
    update();
    const timer = window.setInterval(update, 250);
    return () => window.clearInterval(timer);
  }, [verificationEndsAt]);

  const phase = status?.phase ?? "idle";
  const category = status?.category ?? suggestion?.category ?? null;
  const searching = ACTIVE_PHASES.has(phase);
  const verifying = phase === "temporary_verification";

  return (
    <section className="w-full rounded-2xl border border-accent/25 bg-accent/[0.055] p-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]">
      <div className="flex items-start justify-between gap-4">
        <div>
          <SectionLabel>Локальный подбор стратегии</SectionLabel>
          <p className="text-sm font-medium text-ink">Adaptive Zapret2</p>
          <p className="mt-1 max-w-xl text-xs leading-relaxed text-ink-muted">
            Локальный подбор для YouTube, Discord и Gaming + GitHub.
            Код Lua не генерируется, данные никуда не отправляются.
          </p>
        </div>
        <Switch
          checked={enabled}
          disabled={searching || verifying}
          onChange={(value) => void patchSettings({ adaptive_strategy_enabled: value })}
        />
      </div>

      {enabled ? (
        <div className="mt-4 border-t border-white/[0.07] pt-4">
          <div className="mb-4 grid gap-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-2xs text-ink-muted">Режим поиска</span>
              <div className="flex flex-wrap gap-1.5">
                {SEARCH_MODES.map((mode) => (
                  <Chip
                    key={mode.key}
                    label={mode.label}
                    active={searchMode === mode.key}
                    disabled={searching || verifying}
                    onClick={() => void patchSettings({ adaptive_search_mode: mode.key })}
                  />
                ))}
              </div>
            </div>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-2xs text-ink-muted">Транспорт</span>
              <div className="flex gap-1.5">
                {(["tls", "quic"] as const).map((transport) => (
                  <Chip
                    key={transport}
                    label={transport.toUpperCase()}
                    active={searchTransport === transport}
                    disabled={searching || verifying}
                    onClick={() => setSearchTransport(transport)}
                  />
                ))}
              </div>
            </div>
          </div>
          {searching || verifying ? (
            <RecoverySession
              status={status}
              remaining={remaining}
              probe={probe}
              busy={busy}
              onCancel={() => void cancel()}
              onConfirm={() => void confirm()}
              onReject={() => void reject()}
            />
          ) : (
            <div className="flex flex-wrap items-end justify-between gap-3">
              <div>
                <p className="text-xs text-ink-soft">
                  {phase === "suggested" && category
                    ? `Глаза заметили повторный сбой: ${CATEGORY_LABEL[category]}`
                    : phase === "exhausted"
                      ? status?.sessionMode === "recovery"
                        ? "Рабочая QUIC-стратегия не найдена. Возвращена исходная конфигурация."
                        : "Новая отличающаяся стратегия не найдена. Работает исходная база."
                      : phase === "probe_unreliable"
                        ? "Среду проверки нельзя надёжно измерить — кандидаты не запускались."
                        : phase === "quic_targets_unavailable"
                          ? "Выбранные endpoints не подтвердили поддержку HTTP/3."
                        : phase === "base_unhealthy"
                          ? "После возврата база перестала проходить проверку. Поиск остановлен."
                          : phase === "internal_error"
                            ? "Не удалось безопасно восстановить исходный runtime."
                            : phase === "applied" && category
                        ? `Стратегия для ${CATEGORY_LABEL[category]} подтверждена.`
                        : "Запустите поиск, если сайт не открывается или видео не загружается."}
                </p>
                {savedCandidateId ? (
                  <p className="mt-1 font-mono text-3xs text-ok">
                    Сохранено для этой сети · {savedCandidateId.slice(-8)}
                  </p>
                ) : null}
              </div>
              <div className="flex flex-wrap gap-2">
                {savedCandidateId && category ? (
                  <Button
                    variant="ghost"
                    disabled={busy}
                    onClick={() => void resetSaved(category)}
                  >
                    Сбросить сохранённую
                  </Button>
                ) : null}
                <Button
                  variant={category === "youtube_twitch" ? "primary" : "ghost"}
                  disabled={!dpiActive || busy || !selectedCategories.includes("youtube_twitch")}
                  onClick={() => void startSearch("youtube_twitch", searchTransport)}
                >
                  Найти для YouTube
                </Button>
                <Button
                  variant={category === "discord" ? "primary" : "ghost"}
                  disabled={!dpiActive || busy || !selectedCategories.includes("discord")}
                  onClick={() => void startSearch("discord", "tls")}
                >
                  Найти для Discord
                </Button>
                <Button
                  variant={category === "gaming" ? "primary" : "ghost"}
                  disabled={!dpiActive || busy || !selectedCategories.includes("gaming")}
                  onClick={() => void startSearch("gaming", searchTransport)}
                >
                  Найти для Gaming + GitHub
                </Button>
              </div>
            </div>
          )}

          {!dpiActive ? (
            <p className="mt-3 text-2xs text-warn">
              Сначала запустите Zapret2. Во время поиска будут краткие переподключения.
            </p>
          ) : null}
          {error ? <p className="mt-3 text-xs text-danger">{error}</p> : null}
        </div>
      ) : (
        <p className="mt-4 border-t border-white/[0.07] pt-3 text-2xs text-ink-muted">
          Выключено по умолчанию до ручной проверки. Включение не запускает поиск само.
        </p>
      )}
    </section>
  );
}

function RecoverySession({
  status,
  remaining,
  probe,
  busy,
  onCancel,
  onConfirm,
  onReject,
}: {
  status: ReturnType<typeof useAdaptiveStrategyStore.getState>["status"];
  remaining: number;
  probe: ReturnType<typeof useAdaptiveStrategyStore.getState>["probe"];
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  onReject: () => void;
}) {
  if (!status) return null;
  const preparing =
    status.phase === "discovering_quic" || status.phase === "calibrating";
  const current = preparing ? (status.currentRound ?? 0) : (status.candidateIndex ?? 0);
  const total = preparing ? (status.totalRounds ?? 3) : (status.candidateTotal ?? 12);
  const verifying = status.phase === "temporary_verification";
  const label = status.category ? CATEGORY_LABEL[status.category] : "сервис";
  const stageLabel =
    status.phase === "discovering_quic"
      ? "Проверяем поддержку HTTP/3"
      : status.phase === "calibrating"
        ? "Калибровка " + (status.transport ?? "tls").toUpperCase()
        : status.sessionMode === "recovery"
          ? "Восстанавливаем " + label
          : "Ищем стратегию для " + label;

  return (
    <div>
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="font-medium text-ink">
          {verifying ? "Проверьте " + label + " вручную" : stageLabel}
        </span>
        <span className="font-mono text-ink-muted">
          {verifying
            ? `${remaining} с`
            : `${current} / ${total} · ${(status.transport ?? "tls").toUpperCase()}`}
        </span>
      </div>

      <div
        className="mt-3 grid grid-cols-12 gap-1"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={total}
        aria-valuenow={current}
        aria-label={preparing ? "Раунды калибровки" : "Проверенные кандидаты"}
      >
        {Array.from({ length: total }, (_, index) => (
          <span
            key={index}
            className={`h-1.5 rounded-full transition-colors ${
              index < current
                ? verifying
                  ? "bg-ok"
                  : "bg-accent"
                : "bg-white/10"
            }`}
          />
        ))}
      </div>

      {probe ? (
        <div className="mt-3 flex flex-wrap gap-x-3 gap-y-1">
          {probe.targets.map((target) => (
            <span key={target.host} className="flex items-center gap-1.5 text-3xs text-ink-muted">
              <span
                className={`h-1.5 w-1.5 rounded-full ${
                  target.httpsOk && (target.transport === "tls" ? target.tlsOk : target.quicOk)
                    ? "bg-ok"
                    : "bg-danger"
                }`}
              />
              {target.host} · {target.latencyMs} мс
            </span>
          ))}
        </div>
      ) : null}

      {status.failureStage && status.failureStage !== "none" ? (
        <p className="mt-2 font-mono text-3xs text-danger">
          Причина: {status.failureStage}
        </p>
      ) : null}

      <p className="mt-3 text-2xs leading-relaxed text-warn">
        {verifying
          ? "Откройте сайт, запустите видео или отправьте сообщение. Сохранение доступно только по вашему подтверждению."
          : preparing
            ? "Сначала определяем проверяемые endpoints и измеряем исходный транспорт. Эту стадию можно отменить."
            : status.sessionMode === "recovery"
              ? "Исходный QUIC не работает. Проверяем отличающиеся стратегии с точным возвратом после каждой попытки."
              : "winws2 кратко перезапускается между кандидатами. Исходная конфигурация сохранена для точного возврата."}
      </p>

      <div className="mt-3 flex flex-wrap gap-2">
        {verifying ? (
          <>
            <Button variant="primary" disabled={busy} onClick={onConfirm}>
              Работает — сохранить
            </Button>
            <Button variant="ghost" disabled={busy} onClick={onReject}>
              Не работает — продолжить
            </Button>
          </>
        ) : null}
        <Button variant="danger" disabled={busy} onClick={onCancel}>
          Отмена и возврат
        </Button>
      </div>
    </div>
  );
}
