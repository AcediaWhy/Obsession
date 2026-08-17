import { useEffect } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useShallow } from "zustand/react/shallow";

import { TRANSITION_WATCHDOG_MS, useDpiStore } from "../store/dpiStore";
import { useAdaptiveStrategyStore } from "../store/adaptiveStrategyStore";
import { useSettingsStore } from "../store/settingsStore";
import { GlassPanel } from "../design/components/GlassPanel";
import { HeroCore } from "../design/components/HeroCore";
import { Parallax } from "../design/parallax";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import { LogStream } from "../design/components/LogStream";
import { Diagnostics } from "../design/components/Diagnostics";
import { LegacyReliabilityPanel } from "../design/components/LegacyReliabilityPanel";
import { Zapret2StrategyPanel } from "../design/components/Zapret2StrategyPanel";
import {
  Button,
  Chip,
  Select,
  SectionLabel,
  StatusBadge,
} from "../design/components/atoms";
import { Uptime } from "../design/components/Uptime";
import { Icon } from "../design/components/icons";
import { spring } from "../design/tokens";

const LEGACY_CATEGORY_LABELS: Record<string, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube / Twitch",
  gaming: "Gaming",
  universal: "Universal",
  atrisk: "Под угрозой",
};

const ZAPRET2_CATEGORY_LABELS: Record<string, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube",
  gaming: "Gaming + GitHub",
};

const CATEGORY_ORDER = ["discord", "youtube_twitch", "gaming", "universal", "atrisk"];

export function DpiScreen() {
  const protectedDpiAvailable = useSettingsStore(
    (state) => state.protectedDpiAvailable,
  );
  const legacyToolsAvailable = useSettingsStore(
    (state) => state.protectedRuntimeAvailable,
  );
  const legacyReliabilityAvailable = useSettingsStore(
    (state) => state.protectedLegacyReliabilityAvailable,
  );
  const s = useDpiStore(useShallow((state) => ({
    active: state.active,
    transitioning: state.transitioning,
    startedAt: state.startedAt,
    processes: state.processes,
    engines: state.engines,
    config: state.config,
    selectedCategories: state.selectedCategories,
    selectedConfigs: state.selectedConfigs,
    zapret2Profiles: state.zapret2Profiles,
    testing: state.testing,
    testingLabel: state.testingLabel,
    testCancel: state.testCancel,
    testResults: state.testResults,
    netStats: state.netStats,
    error: state.error,
    reconcileTransition: state.reconcileTransition,
    start: state.start,
    stop: state.stop,
    setEngine: state.setEngine,
    toggleCategory: state.toggleCategory,
    setConfig: state.setConfig,
    autoConfigure: state.autoConfigure,
    testAll: state.testAll,
    cancelTest: state.cancelTest,
  })));
  const adaptivePhase = useAdaptiveStrategyStore((state) => state.status?.phase ?? "idle");
  const adaptiveBusy = ![
    "idle",
    "suggested",
    "applied",
    "exhausted",
    "cancelled",
  ].includes(adaptivePhase);
  const busy = s.transitioning || s.testing || adaptiveBusy;
  const zapret2Selected = s.engines.some(
    (engine) => engine.kind === "zapret2" && engine.selected,
  );
  const categoryLabels = zapret2Selected
    ? ZAPRET2_CATEGORY_LABELS
    : LEGACY_CATEGORY_LABELS;
  const categories = (s.config?.categories ?? [])
    .filter((category) => !zapret2Selected || category in ZAPRET2_CATEGORY_LABELS)
    .sort((a, b) => CATEGORY_ORDER.indexOf(a) - CATEGORY_ORDER.indexOf(b));

  useEffect(() => {
    if (!s.transitioning) return;
    const timer = window.setTimeout(
      () => void s.reconcileTransition(),
      TRANSITION_WATCHDOG_MS,
    );
    return () => window.clearTimeout(timer);
  }, [s.reconcileTransition, s.transitioning]);

  return (
    <div className="flex h-full flex-col gap-4">
      {/* Заголовок. */}
      <StaggerItem standalone className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">DPI-обход</h1>
          <p className="text-sm text-ink-muted">
            Обход блокировок через Zapret (winws)
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Uptime active={s.active} startedAt={s.startedAt} />
          <StatusBadge active={s.active} />
        </div>
      </StaggerItem>

      <div className="screen-split screen-split--end-360">
        {/* Левая колонка: питание + категории. */}
        <GlassPanel scroll>
          <Stagger className="flex flex-col items-center gap-6">
            <StaggerItem className="mt-2 flex flex-col items-center gap-4">
              <Parallax depth={18}>
                <HeroCore
                  active={s.active}
                  busy={s.transitioning || !protectedDpiAvailable}
                  scanning={s.testing}
                  alarm={s.testing && Object.values(s.testResults).some((v) => !v)}
                  onClick={() => {
                    if (s.active) void s.stop();
                    else if (protectedDpiAvailable) void s.start();
                  }}
                />
              </Parallax>
              <div className="text-center text-sm text-ink-soft">
                {!protectedDpiAvailable
                  ? "Защищённая служба DPI недоступна"
                  : s.active
                  ? s.processes.length > 0
                    ? `Обход активен · ${s.processes.length} процесс(ов)`
                    : "Обход активен · системная служба"
                  : "Активируйте ядро"}
              </div>
            </StaggerItem>

            {!protectedDpiAvailable && (
              <StaggerItem className="w-full">
                <div className="w-full rounded-xl border border-warn/40 bg-warn/10 px-3 py-2 text-xs leading-relaxed text-warn">
                  Не удалось подтвердить защищённую службу ObsessionRuntime или её
                  DPI-capability. Компоненты из AppData по-прежнему не запускаются.
                </div>
              </StaggerItem>
            )}

            {s.error && (
              <StaggerItem className="w-full">
                <div className="w-full rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
                  {s.error}
                </div>
              </StaggerItem>
            )}

            {/* Движок DPI: Zapret1 (Legacy) / Zapret2 (Beta). */}
            {s.engines.length > 0 && (
              <StaggerItem className="w-full">
                <SectionLabel>Движок</SectionLabel>
                <div className="flex flex-wrap gap-2">
                  {s.engines.map((e) => (
                    <Chip
                      key={e.kind}
                      label={
                        (e.kind === "zapret2" ? "Zapret2" : "Zapret Legacy") +
                        ` · v${e.version}` +
                        (e.beta ? " · Beta" : "") +
                        (e.available ? "" : " (пока недоступен)")
                      }
                      active={e.selected}
                      disabled={s.active || !e.available}
                      onClick={() => s.setEngine(e.kind)}
                    />
                  ))}
                </div>
              </StaggerItem>
            )}

            {/* Категории. */}
            <StaggerItem className="w-full">
              <SectionLabel>Категории</SectionLabel>
              <div className="flex flex-wrap gap-2">
                {categories.map((cat) => (
                  <Chip
                    key={cat}
                    label={categoryLabels[cat] ?? cat}
                    active={s.selectedCategories.includes(cat)}
                    disabled={s.active}
                    onClick={() => s.toggleCategory(cat)}
                  />
                ))}
              </div>
            </StaggerItem>

            <StaggerItem className="w-full">
              {zapret2Selected ? (
                <Zapret2StrategyPanel profiles={s.zapret2Profiles} />
              ) : (
                <>
                <SectionLabel>Конфигурации Legacy</SectionLabel>
                <motion.div layout className="flex w-full flex-col gap-3">
                  <AnimatePresence initial={false}>
                    {s.selectedCategories.map((cat) => {
                      const files = s.config?.configs[cat] ?? [];
                      const current = s.selectedConfigs[cat] ?? "";
                      const result = s.testResults[current];
                      return (
                        <motion.div
                          key={cat}
                          layout
                          initial={{ opacity: 0, height: 0, y: -6 }}
                          animate={{ opacity: 1, height: "auto", y: 0 }}
                          exit={{ opacity: 0, height: 0, y: -6 }}
                          transition={spring.expand}
                          className="flex flex-col gap-1.5 overflow-hidden"
                        >
                          <div className="flex items-center justify-between">
                            <span className="text-xs font-medium text-ink-soft">
                              {LEGACY_CATEGORY_LABELS[cat] ?? cat}
                            </span>
                            {result !== undefined ? (
                              <span
                                className={`text-2xs font-semibold ${
                                  result ? "text-ok" : "text-danger"
                                }`}
                              >
                                {result ? "работает" : "не прошёл"}
                              </span>
                            ) : null}
                          </div>
                          <Select
                            value={current}
                            options={files}
                            disabled={s.active}
                            onChange={(value) => s.setConfig(cat, value)}
                          />
                          {s.netStats[cat]?.conf === current && current ? (
                            <span className="text-3xs text-ok">
                              работал {s.netStats[cat].success_count} раз
                            </span>
                          ) : null}
                        </motion.div>
                      );
                    })}
                  </AnimatePresence>
                </motion.div>
                </>
              )}
            </StaggerItem>

            {!zapret2Selected ? (
              <>
            {/* Тестирование / авто-подбор. Во время теста кнопки превращаются в
                «Отмена» — тест можно прервать и сразу пользоваться обходом. */}
            <StaggerItem className="flex w-full gap-2">
              {s.testing ? (
                <Button
                  variant="danger"
                  disabled={s.testCancel}
                  onClick={() => s.cancelTest()}
                  className="flex-1"
                >
                  <span className="block truncate">
                    {s.testCancel
                      ? "Отмена…"
                      : `Отменить${s.testingLabel ? ` · ${s.testingLabel}` : " тест"}`}
                  </span>
                </Button>
              ) : (
                <>
                  <Button
                    variant="ghost"
                    disabled={busy || s.active || !legacyToolsAvailable}
                    onClick={() => s.autoConfigure()}
                    className="flex-1"
                  >
                    Авто-подбор
                  </Button>
                  <Button
                    variant="ghost"
                    disabled={busy || s.active || !legacyToolsAvailable}
                    onClick={() => s.testAll()}
                  >
                    <span className="flex items-center gap-1.5">
                      <Icon.Refresh size={15} /> Тест
                    </span>
                  </Button>
                </>
              )}
            </StaggerItem>
              </>
            ) : null}

            {/* Диагностика доступности (работает ли обход). */}
            <StaggerItem className="w-full">
              <Diagnostics />
            </StaggerItem>

            {/* Систему восстановления пока показываем только для Zapret Legacy. */}
            {!zapret2Selected && legacyReliabilityAvailable && (
              <StaggerItem className="w-full">
                <LegacyReliabilityPanel />
              </StaggerItem>
            )}
          </Stagger>
        </GlassPanel>

        {/* Правая колонка: лог. */}
        <GlassPanel className="flex flex-col overflow-hidden">
          <LogStream />
        </GlassPanel>
      </div>
    </div>
  );
}
