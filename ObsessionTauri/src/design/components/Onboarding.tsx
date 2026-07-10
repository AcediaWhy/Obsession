import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { useSettingsStore } from "../../store/settingsStore";
import { useThemeStore, THEMES, type Theme } from "../../store/themeStore";
import { useSecretStore } from "../../store/secretStore";
import { GlassPanel } from "./GlassPanel";
import { Button } from "./atoms";
import { Icon } from "./icons";
import { AuroraCore } from "./AuroraCore";
import { RainCore } from "./RainCore";
import { OphanimCore } from "./OphanimCore";
import { FallenCore } from "./FallenCore";
import { FirefliesCore } from "./FirefliesCore";
import { HearthCore } from "./HearthCore";
import { spring } from "../tokens";

// Приветственный онбординг при первом запуске. Показывается, пока
// settings.has_completed_onboarding === false; по «Готово»/«Пропустить»
// выставляет флаг (персист в Rust) и уходит навсегда.
//
// Шаги: приветствие → преимущества → выбор темы → авторство.

const APP_VERSION = "1.0.0";

type StepDef = { id: string; render: () => JSX.Element };

// ─── Преимущества (факты из реальных фич приложения) ──────────────────────────
const FEATURES: { icon: (p: { size?: number }) => JSX.Element; title: string; desc: string }[] = [
  { icon: Icon.Bolt, title: "DPI-обход", desc: "Обход блокировок через Zapret/winws: Discord, YouTube, игры и другое" },
  { icon: Icon.Robot, title: "ИИ-разблокировка", desc: "Доступ к ИИ-сервисам через подмену hosts (Malw / GeoHide)" },
  { icon: Icon.Send, title: "Telegram-прокси", desc: "Свой прокси для Telegram одной кнопкой, ссылка для подключения" },
  { icon: Icon.Refresh, title: "Авто-восстановление", desc: "Следит за обходом и сам переключает стратегию, если он отвалился" },
  { icon: Icon.List, title: "Списки и профили", desc: "Свои домены и наборы настроек под разные ситуации" },
  { icon: Icon.Layers, title: "Живые темы", desc: "Несколько атмосферных оформлений с анимированным ядром" },
];

function ThemeMiniCore({ id, active }: { id: Theme; active: boolean }) {
  const size = 96;
  const noop = () => {};
  if (id === "ophanim") return <OphanimCore active={active} onClick={noop} size={size} />;
  if (id === "fallendown") return <FallenCore active={active} onClick={noop} size={size} />;
  if (id === "fireflies") return <FirefliesCore active={active} onClick={noop} size={size} />;
  if (id === "hearth") return <HearthCore active={active} onClick={noop} size={size} />;
  if (id === "japan") return <RainCore active={active} onClick={noop} size={size} />;
  return <AuroraCore active={active} onClick={noop} size={size} />;
}

export function Onboarding() {
  const patch = useSettingsStore((s) => s.patch);
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const unlocked = useSecretStore((s) => s.unlocked);

  const [step, setStep] = useState(0);
  const [closing, setClosing] = useState(false);

  // Только открытые темы (секретные пасхалки в онбординге не светим).
  const themeOptions = THEMES.filter((th) => !th.secret || unlocked.includes(th.secret));

  const finish = () => {
    setClosing(true);
    // Персист флага — best-effort; UI уходит сразу после анимации выхода.
    void patch({ has_completed_onboarding: true });
  };

  const steps: StepDef[] = [
    // 1 · Приветствие
    {
      id: "welcome",
      render: () => (
        <div className="flex flex-col items-center gap-6 text-center">
          <motion.div
            initial={{ scale: 0.8, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={spring.soft}
            className="flex h-20 w-20 items-center justify-center rounded-2xl bg-gradient-to-br from-accent to-accent-violet shadow-glow"
          >
            <Icon.Bolt size={40} />
          </motion.div>
          <div>
            <h2 className="font-display text-3xl font-bold text-gradient">Добро пожаловать в Obsession</h2>
            <p className="mx-auto mt-3 max-w-md text-sm text-ink-soft">
              Единый инструмент для обхода блокировок: DPI, ИИ-сервисы и Telegram.
              Пара минут — и всё готово к работе.
            </p>
          </div>
        </div>
      ),
    },
    // 2 · Преимущества
    {
      id: "features",
      render: () => (
        <div className="flex flex-col gap-5">
          <div className="text-center">
            <h2 className="font-display text-2xl font-bold text-gradient">Что умеет приложение</h2>
            <p className="mt-2 text-sm text-ink-muted">Всё в одном месте, без ручной возни с конфигами</p>
          </div>
          <div className="grid grid-cols-2 gap-3">
            {FEATURES.map((f, i) => (
              <motion.div
                key={f.title}
                initial={{ opacity: 0, y: 12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ ...spring.soft, delay: 0.05 * i }}
                className="flex gap-3 rounded-xl border border-glass-border bg-white/5 p-3"
              >
                <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent/15 text-accent-cyan">
                  <f.icon size={18} />
                </div>
                <div className="min-w-0">
                  <div className="text-sm font-semibold text-ink">{f.title}</div>
                  <div className="mt-0.5 text-xs text-ink-muted">{f.desc}</div>
                </div>
              </motion.div>
            ))}
          </div>
        </div>
      ),
    },
    // 3 · Выбор темы
    {
      id: "theme",
      render: () => (
        <div className="flex flex-col gap-5">
          <div className="text-center">
            <h2 className="font-display text-2xl font-bold text-gradient">Выберите оформление</h2>
            <p className="mt-2 text-sm text-ink-muted">Можно сменить в любой момент в Настройках</p>
          </div>
          <div className="flex flex-wrap justify-center gap-3">
            {themeOptions.map((th) => {
              const selected = theme === th.id;
              return (
                <button
                  key={th.id}
                  onClick={() => setTheme(th.id)}
                  className={[
                    "no-drag group relative flex flex-col items-center gap-2 rounded-xl border p-3 transition-all",
                    selected
                      ? "border-accent/60 bg-accent/10 shadow-glow"
                      : "border-glass-border bg-white/5 hover:bg-white/10",
                  ].join(" ")}
                >
                  <div className="pointer-events-none flex h-[96px] w-[96px] items-center justify-center">
                    <ThemeMiniCore id={th.id} active={selected} />
                  </div>
                  <div className="flex items-center gap-1.5">
                    {selected && <Icon.Check size={14} />}
                    <span className={`text-sm font-semibold ${selected ? "text-ink" : "text-ink-soft"}`}>
                      {th.label}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      ),
    },
    // 4 · Авторство
    {
      id: "author",
      render: () => (
        <div className="flex flex-col items-center gap-6 text-center">
          <motion.div
            initial={{ scale: 0.8, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={spring.soft}
            className="flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-accent to-accent-violet shadow-glow"
          >
            <Icon.Bolt size={32} />
          </motion.div>
          <div>
            <h2 className="font-display text-2xl font-bold text-gradient">Готово к запуску</h2>
            <p className="mx-auto mt-3 max-w-sm text-sm text-ink-soft">
              Всё настроено. Начните с экрана «DPI-обход» — выберите категорию и включите ядро.
            </p>
          </div>
          <div className="rounded-xl border border-glass-border bg-white/5 px-5 py-3">
            <div className="text-sm font-semibold text-ink">made by VlarpSu</div>
            <div className="mt-0.5 text-xs text-ink-muted">Obsession · v{APP_VERSION}</div>
          </div>
        </div>
      ),
    },
  ];

  const isLast = step === steps.length - 1;
  const next = () => (isLast ? finish() : setStep((s) => s + 1));
  const back = () => setStep((s) => Math.max(0, s - 1));

  return (
    <AnimatePresence>
      {!closing && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.28, ease: "easeOut" }}
          className="fixed inset-0 z-50 flex items-center justify-center bg-base/70 backdrop-blur-sm"
        >
          <GlassPanel spotlight={false} className="w-[620px] max-w-[92vw] p-8">
            {/* Тело шага. */}
            <div className="min-h-[320px]">
              <AnimatePresence mode="wait">
                <motion.div
                  key={steps[step].id}
                  initial={{ opacity: 0, x: 24 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: -24 }}
                  transition={{ duration: 0.24, ease: "easeOut" }}
                >
                  {steps[step].render()}
                </motion.div>
              </AnimatePresence>
            </div>

            {/* Точки-прогресс + навигация. */}
            <div className="mt-8 flex items-center justify-between">
              <div className="flex gap-1.5">
                {steps.map((_, i) => (
                  <span
                    key={i}
                    className={[
                      "h-1.5 rounded-full transition-all",
                      i === step ? "w-5 bg-accent" : "w-1.5 bg-white/15",
                    ].join(" ")}
                  />
                ))}
              </div>

              <div className="flex items-center gap-2">
                {step > 0 && (
                  <Button variant="ghost" onClick={back}>
                    Назад
                  </Button>
                )}
                <Button variant="primary" onClick={next}>
                  {isLast ? "Готово" : "Далее"}
                </Button>
              </div>
            </div>

            {/* Пропуск — на каждом шаге, кроме последнего (там уже «Готово»). */}
            {!isLast && (
              <button
                onClick={finish}
                className="no-drag mt-4 w-full text-center text-xs text-ink-muted transition-colors hover:text-ink-soft"
              >
                Пропустить
              </button>
            )}
          </GlassPanel>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

