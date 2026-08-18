import { useEffect, useMemo, useState } from "react";

import type {
  AdaptiveCategory,
  AdaptivePhase,
  AdaptiveProbeBatch,
  AdaptiveStatus,
  AdaptiveTransport,
  Zapret2ProfileDescriptor,
} from "../../lib/tauri";
import { useAdaptiveStrategyStore } from "../../store/adaptiveStrategyStore";
import { useDpiStore } from "../../store/dpiStore";
import { useSettingsStore } from "../../store/settingsStore";
import { Button, Chip, SectionLabel, Switch } from "./atoms";
import { Collapse } from "./Collapse";
import { Icon } from "./icons";

const CATEGORY_ORDER: AdaptiveCategory[] = ["discord", "youtube_twitch", "gaming"];

const CATEGORY_LABEL: Record<AdaptiveCategory, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube",
  gaming: "Gaming + GitHub",
};

const SEARCH_MODES = [
  { key: "fast", label: "Быстро" },
  { key: "balanced", label: "Баланс" },
  { key: "deep", label: "Глубоко" },
] as const;

const ACTIVE_PHASES = new Set<AdaptivePhase>([
  "discovering_quic",
  "calibrating",
  "searching",
  "candidate_probe",
  "rolling_back",
  "applying",
]);

type Props = {
  profiles: Zapret2ProfileDescriptor[];
};

export function Zapret2StrategyPanel({ profiles }: Props) {
  const enabled = useSettingsStore(
    (state) => state.settings?.adaptive_strategy_enabled ?? false,
  );
  const patchSettings = useSettingsStore((state) => state.patch);
  const searchMode = useSettingsStore(
    (state) => state.settings?.adaptive_search_mode ?? "balanced",
  );
  const dpiActive = useDpiStore((state) => state.active);
  const selectedCategories = useDpiStore((state) => state.selectedCategories);
  const loadZapret2Profiles = useDpiStore((state) => state.loadZapret2Profiles);
  const status = useAdaptiveStrategyStore((state) => state.status);
  const probe = useAdaptiveStrategyStore((state) => state.probe);
  const recommendation = useAdaptiveStrategyStore((state) => state.recommendation);
  const recommendationCheckedFor = useAdaptiveStrategyStore(
    (state) => state.recommendationCheckedFor,
  );
  const busy = useAdaptiveStrategyStore((state) => state.busy);
  const error = useAdaptiveStrategyStore((state) => state.error);
  const verificationEndsAt = useAdaptiveStrategyStore(
    (state) => state.verificationEndsAt,
  );
  const startSearch = useAdaptiveStrategyStore((state) => state.startSearch);
  const findRecommendation = useAdaptiveStrategyStore((state) => state.findRecommendation);
  const applyRecommendation = useAdaptiveStrategyStore((state) => state.applyRecommendation);
  const cancel = useAdaptiveStrategyStore((state) => state.cancel);
  const confirm = useAdaptiveStrategyStore((state) => state.confirm);
  const reject = useAdaptiveStrategyStore((state) => state.reject);
  const resetSaved = useAdaptiveStrategyStore((state) => state.resetSaved);

  const [setupCategory, setSetupCategory] = useState<AdaptiveCategory | null>(null);
  const [transport, setTransport] = useState<AdaptiveTransport>("tls");
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [remaining, setRemaining] = useState(60);

  useEffect(() => {
    if (!verificationEndsAt) return;
    const update = () =>
      setRemaining(Math.max(0, Math.ceil((verificationEndsAt - Date.now()) / 1000)));
    update();
    const timer = window.setInterval(update, 250);
    return () => window.clearInterval(timer);
  }, [verificationEndsAt]);
  useEffect(() => {
    if (
      status?.phase === "applied" ||
      status?.phase === "exhausted" ||
      status?.phase === "cancelled" ||
      status?.phase === "base_unhealthy"
    ) {
      void loadZapret2Profiles();
    }
  }, [loadZapret2Profiles, status?.phase, status?.candidateId]);


  const phase = status?.phase ?? "idle";
  const searching = ACTIVE_PHASES.has(phase);
  const verifying = phase === "temporary_verification";
  const sessionActive = searching || verifying;
  const services = CATEGORY_ORDER.filter((category) => selectedCategories.includes(category));
  const profilesByCategory = useMemo(() => {
    const grouped = new Map<AdaptiveCategory, Zapret2ProfileDescriptor[]>();
    for (const category of CATEGORY_ORDER) grouped.set(category, []);
    for (const profile of profiles) {
      if (CATEGORY_ORDER.includes(profile.category as AdaptiveCategory)) {
        grouped.get(profile.category as AdaptiveCategory)?.push(profile);
      }
    }
    return grouped;
  }, [profiles]);

  const openSetup = (category: AdaptiveCategory) => {
    setTransport("tls");
    setSetupCategory((current) => (current === category ? null : category));
  };
  const resetProfile = async (category: AdaptiveCategory) => {
    await resetSaved(category);
    await loadZapret2Profiles();
  };
  const applyGamingRecommendation = async (selectedTransport: AdaptiveTransport) => {
    await applyRecommendation("gaming", selectedTransport);
    await loadZapret2Profiles();
  };

  return (
    <section className="w-full">
      <div className="mb-2 flex items-center justify-between gap-4">
        <SectionLabel>Сервисы Zapret2</SectionLabel>
        <label className="flex items-center gap-2 text-2xs text-ink-muted">
          <span>Локальный подбор</span>
          <Switch
            checked={enabled}
            disabled={sessionActive}
            onChange={(value) => void patchSettings({ adaptive_strategy_enabled: value })}
          />
        </label>
      </div>

      <div className="overflow-hidden rounded-lg border border-white/[0.08]">
        {services.map((category) => {
          const categoryProfiles = profilesByCategory.get(category) ?? [];
          const ownsSession = status?.category === category && sessionActive;
          const gamingRecoveryAvailable =
            category === "gaming" &&
            status?.phase === "suggested" &&
            status.category === "gaming";
          const expanded = ownsSession || setupCategory === category;
          const terminalMessage = status?.category === category ? phaseMessage(status) : null;
          const selectedControlProfile = categoryProfiles.find(
            (profile) => !profile.dataPlane && profile.transport === transport,
          );
          return (
            <div
              key={category}
              className="border-b border-white/[0.06] last:border-b-0"
            >
              <div className="flex min-h-16 items-center justify-between gap-3 px-3 py-2.5">
                <div className="min-w-0">
                  <div className="text-sm font-medium text-ink">{CATEGORY_LABEL[category]}</div>
                  <div className="mt-1 flex flex-wrap gap-1.5">
                    <ProfileBadges profiles={categoryProfiles} />
                  </div>
                </div>
                {!ownsSession ? (
                  <Button
                    variant="ghost"
                    disabled={sessionActive || busy}
                    onClick={() => openSetup(category)}
                    className="shrink-0 px-3 py-1.5 text-xs"
                  >
                    <span className="flex items-center gap-1.5">
                      <Icon.Refresh size={14} /> Подобрать
                    </span>
                  </Button>
                ) : (
                  <span className="shrink-0 text-2xs font-medium text-accent-cyan">
                    Выполняется
                  </span>
                )}
              </div>

              <Collapse open={expanded}>
                    <div className="border-t border-white/[0.06] bg-white/[0.025] px-3 py-3">
                      {ownsSession && status ? (
                        <SearchSession
                          status={status}
                          remaining={remaining}
                          probe={probe}
                          busy={busy}
                          onCancel={() => void cancel()}
                          onConfirm={() => void confirm()}
                          onReject={() => void reject()}
                        />
                      ) : (
                        category === "gaming" ? (
                          <div className="flex flex-col gap-3">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-2xs text-ink-muted">Транспорт</span>
                              {(["tls", "quic"] as const).map((value) => (
                                <Chip
                                  key={value}
                                  label={value.toUpperCase()}
                                  active={transport === value}
                                  disabled={busy}
                                  onClick={() => setTransport(value)}
                                />
                              ))}
                            </div>
                            <p className="text-2xs leading-relaxed text-ink-muted">
                              {!enabled
                                ? "Включите локальный подбор"
                                : !dpiActive
                                  ? "Сначала запустите Zapret2"
                                  : recommendation?.transport === transport
                                    ? recommendation.recommendationReason
                                    : selectedControlProfile?.trust === "confirmed"
                                      ? "Стратегия подтверждена для этой сети"
                                      : selectedControlProfile?.trust === "recommended"
                                        ? selectedControlProfile.recommendationReason
                                        : recommendationCheckedFor === transport
                                          ? "Недостаточно подтверждённых данных этой сети"
                                          : "IPSet-профиль подготовлен; эффективность на блокировке не проверялась"}
                            </p>
                            <div className="flex flex-wrap justify-end gap-2">
                              {gamingRecoveryAvailable ? (
                                <Button
                                  variant="danger"
                                  disabled={!enabled || !dpiActive || busy}
                                  onClick={() => void startSearch("gaming", transport)}
                                  className="px-3 py-1.5 text-xs"
                                >
                                  Восстановить после сбоя
                                </Button>
                              ) : null}
                              <Button
                                variant="ghost"
                                disabled={!enabled || !dpiActive || busy}
                                onClick={() =>
                                  void findRecommendation("gaming", transport)
                                }
                                className="px-3 py-1.5 text-xs"
                              >
                                Подобрать рекомендацию
                              </Button>
                              {recommendation?.transport === transport ? (
                                <Button
                                  variant="primary"
                                  disabled={!enabled || !dpiActive || busy}
                                  onClick={() =>
                                    void applyGamingRecommendation(transport)
                                  }
                                  className="px-3 py-1.5 text-xs"
                                >
                                  Применить
                                </Button>
                              ) : null}
                            </div>
                          </div>
                        ) : (
                          <div className="flex flex-col gap-3">
                            <div className="flex flex-wrap items-center justify-between gap-3">
                              <div className="flex w-full min-w-0 flex-col gap-2">
                                <span className="text-2xs text-ink-muted">Режим</span>
                                <div className="grid w-full min-w-0 grid-cols-3 gap-2">
                                  {SEARCH_MODES.map((mode) => (
                                    <Chip
                                      key={mode.key}
                                      label={mode.label}
                                      active={searchMode === mode.key}
                                      disabled={busy}
                                      onClick={() =>
                                        void patchSettings({ adaptive_search_mode: mode.key })
                                      }
                                    />
                                  ))}
                                </div>
                              </div>
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="text-2xs text-ink-muted">Транспорт</span>
                                {(["tls", "quic"] as const)
                                  .filter(
                                    (value) => category !== "discord" || value === "tls",
                                  )
                                  .map((value) => (
                                    <Chip
                                      key={value}
                                      label={value.toUpperCase()}
                                      active={transport === value}
                                      disabled={busy}
                                      onClick={() => setTransport(value)}
                                    />
                                  ))}
                              </div>
                            </div>
                            <div className="flex items-center justify-between gap-3">
                              <span
                                className={`text-2xs ${
                                  dpiActive ? "text-ink-muted" : "text-warn"
                                }`}
                              >
                                {!enabled
                                  ? "Включите локальный подбор"
                                  : !dpiActive
                                    ? "Сначала запустите Zapret2"
                                    : terminalMessage ??
                                      "Исходная конфигурация будет восстановлена после неудачной попытки"}
                              </span>
                              <Button
                                variant="primary"
                                disabled={!enabled || !dpiActive || busy}
                                onClick={() => void startSearch(category, transport)}
                                className="shrink-0 px-3 py-1.5 text-xs"
                              >
                                Начать
                              </Button>
                            </div>
                          </div>
                        )
                      )}
                    </div>
              </Collapse>
            </div>
          );
        })}
      </div>

      <button
        type="button"
        aria-expanded={detailsOpen}
        onClick={() => setDetailsOpen((open) => !open)}
        className="no-drag btn-anim mt-2 flex w-full items-center justify-between rounded-lg px-2 py-2 text-xs text-ink-muted hover:bg-white/5 hover:text-ink-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70"
      >
        <span className="flex items-center gap-2">
          <Icon.List size={15} /> Технические детали
        </span>
        <span className="flex items-center gap-2">
          <span className="font-mono text-3xs">{profiles.length} профилей</span>
          <Icon.Chevron
            size={14}
            className={`transition-transform duration-[var(--motion-fast)] ${
              detailsOpen ? "rotate-180" : ""
            }`}
          />
        </span>
      </button>

      <Collapse open={detailsOpen}>
            <TechnicalDetails
              profiles={profiles}
              probe={probe}
              busy={busy || sessionActive}
              error={error}
              onReset={(category) => void resetProfile(category)}
            />
      </Collapse>
    </section>
  );
}

function ProfileBadges({ profiles }: { profiles: Zapret2ProfileDescriptor[] }) {
  const controls = profiles.filter((profile) => !profile.dataPlane);
  const badges = ["tls", "quic"]
    .map((transport) => {
      const profile = controls.find((item) => item.transport === transport);
      if (!profile) return null;
      return {
        key: transport,
        label: `${transport.toUpperCase()} · ${trustLabel(profile.trust)}`,
        trust: profile.trust,
      };
    })
    .filter(Boolean) as Array<{
      key: string;
      label: string;
      trust: Zapret2ProfileDescriptor["trust"];
    }>;
  if (profiles.some((profile) => profile.dataPlane)) {
    badges.push({ key: "ipset", label: "IPSet · Подготовлен", trust: "prepared" });
  }
  if (badges.length === 0) {
    return <span className="text-3xs text-danger">Нет активных профилей</span>;
  }
  return badges.map((badge) => (
    <span
      key={badge.key}
      className={`rounded-full border px-2 py-0.5 text-3xs ${
        badge.trust === "confirmed"
          ? "border-ok/30 bg-ok/10 text-ok"
          : badge.trust === "recommended"
            ? "border-accent-cyan/30 bg-accent-cyan/10 text-accent-cyan"
            : "border-white/[0.08] bg-white/[0.04] text-ink-muted"
      }`}
    >
      {badge.label}
    </span>
  ));
}

function trustLabel(trust: Zapret2ProfileDescriptor["trust"]) {
  if (trust === "confirmed") return "Подтверждён";
  if (trust === "recommended") return "Рекомендован";
  return "Подготовлен";
}

function SearchSession({
  status,
  remaining,
  probe,
  busy,
  onCancel,
  onConfirm,
  onReject,
}: {
  status: AdaptiveStatus;
  remaining: number;
  probe: AdaptiveProbeBatch | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  onReject: () => void;
}) {
  const preparing = status.phase === "discovering_quic" || status.phase === "calibrating";
  const verifying = status.phase === "temporary_verification";
  const current = preparing ? (status.currentRound ?? 0) : (status.candidateIndex ?? 0);
  const total = Math.max(1, preparing ? (status.totalRounds ?? 3) : (status.candidateTotal ?? 1));
  const stage =
    status.phase === "discovering_quic"
      ? "Проверяем поддержку HTTP/3"
      : status.phase === "calibrating"
        ? "Проверяем исходную конфигурацию"
        : status.phase === "rolling_back"
          ? "Возвращаем исходную конфигурацию"
          : verifying
            ? "Проверьте сервис вручную"
            : status.sessionMode === "recovery"
              ? "Ищем рабочий вариант"
              : "Сравниваем стратегии";
  const passed = probe?.targets.filter((target) =>
    target.transport === "tls" ? target.tlsOk && target.httpsOk : target.quicOk,
  ).length;

  return (
    <div>
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="font-medium text-ink">{stage}</span>
        <span className="font-mono text-ink-muted">
          {verifying ? `${remaining} с` : `${current} / ${total}`}
        </span>
      </div>
      <div className="mt-2 flex gap-1" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={current}>
        {Array.from({ length: total }, (_, index) => (
          <span
            key={index}
            className={`h-1.5 min-w-0 flex-1 rounded-full ${index < current ? "bg-accent" : "bg-white/10"}`}
          />
        ))}
      </div>
      {probe ? (
        <p className="mt-2 text-3xs text-ink-muted">
          Ответили {passed ?? 0} из {probe.targets.length} · {probe.transport.toUpperCase()}
        </p>
      ) : null}
      <div className="mt-3 flex flex-wrap gap-2">
        {verifying ? (
          <>
            <Button variant="primary" disabled={busy} onClick={onConfirm} className="px-3 py-1.5 text-xs">
              Работает — сохранить
            </Button>
            <Button variant="ghost" disabled={busy} onClick={onReject} className="px-3 py-1.5 text-xs">
              Не работает
            </Button>
          </>
        ) : null}
        <Button variant="danger" disabled={busy} onClick={onCancel} className="px-3 py-1.5 text-xs">
          Отмена и возврат
        </Button>
      </div>
    </div>
  );
}

function TechnicalDetails({ profiles, probe, busy, error, onReset }: {
  profiles: Zapret2ProfileDescriptor[];
  probe: AdaptiveProbeBatch | null;
  busy: boolean;
  error: string;
  onReset: (category: AdaptiveCategory) => void;
}) {
  const savedCategories = new Set(
    profiles.filter((profile) => profile.source === "adaptive").map((profile) => profile.category),
  );
  return (
    <div className="mt-1 border-t border-white/[0.06] px-2 pt-2">
      {profiles.map((profile) => (
        <div key={`${profile.category}:${profile.profileId}`} className="flex items-start justify-between gap-3 border-b border-white/[0.05] py-2 last:border-b-0">
          <div className="min-w-0">
            <div className="truncate font-mono text-3xs text-ink-soft">{profile.profileId}</div>
            <div className="mt-0.5 break-all text-3xs text-ink-muted">
              {CATEGORY_LABEL[profile.category as AdaptiveCategory] ?? profile.category} · {profile.transport.toUpperCase()} / {profile.ports}
              {[profile.hostlist, profile.ipset].filter(Boolean).length ? ` · ${[profile.hostlist, profile.ipset].filter(Boolean).join(" · ")}` : ""}
            </div>
            {profile.recommendationReason ? (
              <div className="mt-0.5 text-3xs text-accent-cyan">
                {profile.recommendationReason}
              </div>
            ) : null}
          </div>
          <span
            className={`shrink-0 text-3xs ${
              profile.trust === "confirmed"
                ? "text-ok"
                : profile.trust === "recommended"
                  ? "text-accent-cyan"
                  : "text-ink-muted"
            }`}
          >
            {trustLabel(profile.trust)}
          </span>
        </div>
      ))}
      {probe ? (
        <div className="border-t border-white/[0.06] py-2">
          <div className="mb-1 text-3xs font-semibold uppercase text-ink-muted">Последняя проверка</div>
          {probe.targets.map((target) => (
            <div key={target.host} className="flex items-center justify-between gap-3 py-1 text-3xs">
              <span className="truncate text-ink-soft">{target.host}</span>
              <span className={target.httpsOk && (target.tlsOk || target.quicOk) ? "text-ok" : "text-danger"}>
                {target.latencyMs} мс · {target.failureStage}
              </span>
            </div>
          ))}
        </div>
      ) : null}
      {savedCategories.size > 0 ? (
        <div className="flex flex-wrap gap-2 border-t border-white/[0.06] py-2">
          {[...savedCategories].map((category) => (
            <Button key={category} variant="ghost" disabled={busy} onClick={() => onReset(category as AdaptiveCategory)} className="px-3 py-1.5 text-xs">
              Сбросить · {CATEGORY_LABEL[category as AdaptiveCategory] ?? category}
            </Button>
          ))}
        </div>
      ) : null}
      {profiles.some((profile) => profile.broadIpset) ? (
        <p className="py-2 text-3xs leading-relaxed text-warn">
          IPSet ограничивает применение desync списком адресов, но не меняет IP или регион.
        </p>
      ) : null}
      {error ? <p className="py-2 text-xs text-danger">{error}</p> : null}
    </div>
  );
}

function phaseMessage(status: AdaptiveStatus): string | null {
  switch (status.phase) {
    case "suggested": return "Обнаружен повторный сбой";
    case "applied": return "Стратегия сохранена для этой сети";
    case "exhausted": return status.sessionMode === "recovery"
      ? "Рабочая стратегия не найдена, исходная конфигурация восстановлена"
      : "Подходящая новая стратегия не найдена";
    case "quic_targets_unavailable": return "Сервисы не подтвердили поддержку HTTP/3";
    case "probe_unreliable": return "Проверку нельзя выполнить надёжно";
    case "base_unhealthy": return "Исходная конфигурация перестала отвечать";
    case "internal_error": return "Не удалось восстановить исходную конфигурацию";
    case "cancelled": return "Поиск отменён, исходная конфигурация восстановлена";
    default: return null;
  }
}
