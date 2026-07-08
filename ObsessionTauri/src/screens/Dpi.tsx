import { AnimatePresence, motion } from "framer-motion";

import { useDpiStore } from "../store/dpiStore";
import { GlassPanel } from "../design/components/GlassPanel";
import { HeroCore } from "../design/components/HeroCore";
import { Parallax } from "../design/parallax";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import { LogStream } from "../design/components/LogStream";
import { Diagnostics } from "../design/components/Diagnostics";
import { BrainPanel } from "../design/components/BrainPanel";
import {
  Button,
  Chip,
  Select,
  SectionLabel,
  StatusBadge,
} from "../design/components/atoms";
import { Uptime } from "../design/components/Uptime";
import { Icon } from "../design/components/icons";

const CATEGORY_LABELS: Record<string, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube / Twitch",
  gaming: "Gaming",
  universal: "Universal",
};

const CATEGORY_ORDER = ["discord", "youtube_twitch", "gaming", "universal"];

export function DpiScreen() {
  const s = useDpiStore();
  const categories = (s.config?.categories ?? []).sort(
    (a, b) => CATEGORY_ORDER.indexOf(a) - CATEGORY_ORDER.indexOf(b),
  );
  const busy = s.transitioning || s.testing;

  return (
    <div className="flex h-full flex-col gap-4">
      {/* Заголовок. */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-bold text-gradient">DPI-обход</h1>
          <p className="text-sm text-ink-muted">
            Обход блокировок через Zapret (winws)
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Uptime active={s.active} />
          <StatusBadge active={s.active} />
        </div>
      </div>

      <div className="grid flex-1 grid-cols-[1fr_360px] gap-4 overflow-hidden">
        {/* Левая колонка: питание + категории. */}
        <GlassPanel className="overflow-y-auto">
          <Stagger className="flex flex-col items-center gap-6">
            <StaggerItem className="mt-2 flex flex-col items-center gap-4">
              <Parallax depth={18}>
                <HeroCore
                  active={s.active}
                  busy={s.transitioning}
                  onClick={() => (s.active ? s.stop() : s.start())}
                />
              </Parallax>
              <div className="text-center text-sm text-ink-soft">
                {s.active
                  ? `Обход активен · ${s.processes.length} процесс(ов)`
                  : "Активируйте ядро"}
              </div>
            </StaggerItem>

            {s.error && (
              <StaggerItem className="w-full">
                <div className="w-full rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
                  {s.error}
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
                    label={CATEGORY_LABELS[cat] ?? cat}
                    active={s.selectedCategories.includes(cat)}
                    disabled={s.active}
                    onClick={() => s.toggleCategory(cat)}
                  />
                ))}
              </div>
            </StaggerItem>

            {/* Конфиги выбранных категорий — плавно раздвигают соседей. */}
            <StaggerItem className="w-full">
              <SectionLabel>Конфигурации</SectionLabel>
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
                        transition={{ type: "spring", stiffness: 320, damping: 30 }}
                        className="flex flex-col gap-1.5 overflow-hidden"
                      >
                        <div className="flex items-center justify-between">
                          <span className="text-xs font-medium text-ink-soft">
                            {CATEGORY_LABELS[cat] ?? cat}
                          </span>
                          {result !== undefined && (
                            <span
                              className={`text-[11px] font-semibold ${result ? "text-ok" : "text-danger"}`}
                            >
                              {result ? "работает" : "не прошёл"}
                            </span>
                          )}
                        </div>
                        <Select
                          value={current}
                          options={files}
                          disabled={s.active}
                          onChange={(v) => s.setConfig(cat, v)}
                        />
                      </motion.div>
                    );
                  })}
                </AnimatePresence>
              </motion.div>
            </StaggerItem>

            {/* Тестирование / авто-подбор. */}
            <StaggerItem className="flex w-full gap-2">
              <Button
                variant="ghost"
                disabled={busy || s.active}
                onClick={() => s.autoConfigure()}
                className="flex-1"
              >
                {s.testing ? s.testingLabel || "Подбор…" : "Авто-подбор"}
              </Button>
              <Button
                variant="ghost"
                disabled={busy || s.active}
                onClick={() => s.selectedCategories[0] && s.testAll(s.selectedCategories[0])}
              >
                <span className="flex items-center gap-1.5">
                  <Icon.Refresh size={15} /> Тест
                </span>
              </Button>
            </StaggerItem>

            {/* Диагностика доступности (работает ли обход). */}
            <StaggerItem className="w-full">
              <Diagnostics />
            </StaggerItem>

            {/* Авто-восстановление (Мозг L3) — debug-читалка статуса. */}
            <StaggerItem className="w-full">
              <BrainPanel />
            </StaggerItem>
          </Stagger>
        </GlassPanel>

        {/* Правая колонка: лог. */}
        <GlassPanel className="flex flex-col overflow-hidden">
          <LogStream height={520} />
        </GlassPanel>
      </div>
    </div>
  );
}
