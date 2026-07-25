import { useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { useSettingsStore } from "../../store/settingsStore";
import { useThemeStore, THEMES, type Theme } from "../../store/themeStore";
import { useSecretStore } from "../../store/secretStore";
import { GlassPanel } from "./GlassPanel";
import { Button } from "./atoms";
import { EyeLogo } from "./EyeLogo";
import { Icon } from "./icons";
import { AuroraCore } from "./AuroraCore";
import { RainLanternCore } from "./RainLanternCore";
import { OphanimCore } from "./OphanimCore";
import { FallenCore } from "./FallenCore";
import { CatnapCore } from "./CatnapCore";
import { MidnightCore } from "./MidnightCore";
import { spring, cascade, dur, ease } from "../tokens";

// Приветственный онбординг при первом запуске. Показывается, пока
// settings.has_completed_onboarding === false; по «Готово»/«Пропустить»
// выставляет флаг (персист в Rust) и уходит навсегда.

type StepDef = {
  id: string;
  kicker: string;
  title: string;
  description: string;
  render: () => JSX.Element;
};

type Feature = {
  icon: (p: { size?: number }) => JSX.Element;
  title: string;
  desc: string;
};

const FEATURES: Feature[] = [
  {
    icon: Icon.Bolt,
    title: "DPI-обход",
    desc: "Готовые профили Zapret/winws для Discord, YouTube, игр и других сервисов.",
  },
  {
    icon: Icon.Robot,
    title: "ИИ-разблокировка",
    desc: "Поддержка hosts-профилей для доступа к ИИ-сервисам без ручной возни.",
  },
  {
    icon: Icon.Send,
    title: "Telegram-прокси",
    desc: "Прокси в один клик и готовая ссылка для подключения на телефоне.",
  },
  {
    icon: Icon.Refresh,
    title: "Авто-восстановление",
    desc: "Следит за состоянием обхода и помогает мягко вернуться к рабочему конфигу.",
  },
  {
    icon: Icon.List,
    title: "Списки и профили",
    desc: "Собственные домены, наборы настроек и быстрый возврат к любимому сценарию.",
  },
  {
    icon: Icon.Layers,
    title: "Живые темы",
    desc: "Несколько атмосферных оформлений с анимированным ядром и мягкими переходами.",
  },
];

function ThemeMiniCore({ id, active }: { id: Theme; active: boolean }) {
  const size = 104;
  const noop = () => {};
  if (id === "ophanim") return <OphanimCore active={active} onClick={noop} size={size} />;
  if (id === "fallendown") return <FallenCore active={active} onClick={noop} size={size} />;
  if (id === "catnap") return <CatnapCore active={active} onClick={noop} size={size} />;
  if (id === "midnight") return <MidnightCore active={active} onClick={noop} size={size} />;
  if (id === "japan") return <RainLanternCore active={active} onClick={noop} size={size} variant="preview" />;
  return <AuroraCore active={active} onClick={noop} size={size} />;
}

function StepChrome({ kicker, title, description }: { kicker: string; title: string; description: string }) {
  return (
    <div className="space-y-3 text-center">
      <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.24em] text-ink-muted">
        <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-glow" />
        {kicker}
      </div>
      <div className="space-y-2">
        <h2 className="font-display text-[2.05rem] font-semibold tracking-tight text-gradient">{title}</h2>
        <p className="mx-auto max-w-[34rem] text-sm leading-6 text-ink-soft">{description}</p>
      </div>
    </div>
  );
}

function FeatureCard({ feature, index }: { feature: Feature; index: number }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 14 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ ...spring.soft, delay: cascade.step * index }}
      className="group relative overflow-hidden rounded-2xl border border-white/10 bg-white/[0.05] p-4 shadow-sm transition-colors hover:border-accent/30 hover:bg-white/[0.07]"
    >
      <div className="absolute inset-0 bg-gradient-to-br from-accent/5 via-transparent to-accent-violet/5 opacity-0 transition-opacity group-hover:opacity-100" />
      <div className="relative flex gap-3">
        <div className="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-accent/20 bg-accent/10 text-accent-cyan shadow-glow/20">
          <feature.icon size={18} />
        </div>
        <div className="min-w-0">
          <div className="text-sm font-semibold text-ink">{feature.title}</div>
          <div className="mt-1 text-xs leading-5 text-ink-muted">{feature.desc}</div>
        </div>
      </div>
    </motion.div>
  );
}

export function Onboarding() {
  const patch = useSettingsStore((s) => s.patch);
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const unlocked = useSecretStore((s) => s.unlocked);

  const [step, setStep] = useState(0);
  const [closing, setClosing] = useState(false);

  const themeOptions = useMemo(
    () => THEMES.filter((th) => !th.secret || unlocked.includes(th.secret)),
    [unlocked],
  );

  const finish = () => {
    setClosing(true);
    void patch({ has_completed_onboarding: true });
  };

  const steps: StepDef[] = [
    {
      id: "welcome",
      kicker: "Первый запуск",
      title: "Добро пожаловать в Obsession",
      description:
        "Один экран, чтобы быстро включить DPI-обход, Telegram-прокси и инструменты для доступа к заблокированным сервисам.",
      render: () => (
        <div className="grid items-center gap-8 lg:grid-cols-[1.08fr_0.92fr]">
          <div className="space-y-6 text-left">
            <div className="space-y-3">
              <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.24em] text-ink-muted">
                <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-glow" />
                быстрый старт
              </div>
              <h3 className="font-display text-4xl font-semibold tracking-tight text-ink">
                Всё, что нужно для обхода, — в одном аккуратном месте.
              </h3>
              <p className="max-w-xl text-sm leading-6 text-ink-soft">
                Obsession помогает включать рабочие конфиги, переключать темы, поднимать прокси и
                не терять время на ручную настройку.
              </p>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              {[
                "готовые профили и списки",
                "автоматическое восстановление",
                "живые темы и мягкие анимации",
                "быстрый переход к рабочему режиму",
              ].map((item, index) => (
                <motion.div
                  key={item}
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ ...spring.soft, delay: 0.03 * index }}
                  className="rounded-2xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-ink-soft"
                >
                  {item}
                </motion.div>
              ))}
            </div>
          </div>

          <div className="relative flex items-center justify-center">
            <div className="absolute inset-x-8 bottom-6 top-8 rounded-[2rem] bg-gradient-to-b from-accent/15 via-accent-violet/10 to-transparent blur-2xl" />
            <motion.div
              initial={{ scale: 0.92, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={spring.soft}
              className="relative rounded-[2rem] border border-white/10 bg-white/[0.045] p-7 shadow-2xl shadow-black/20"
            >
              <div className="absolute inset-0 rounded-[2rem] bg-gradient-to-br from-white/5 via-transparent to-accent/5" />
              <div className="relative flex flex-col items-center gap-5 text-center">
                <div className="rounded-3xl shadow-glow">
                  <EyeLogo size={92} />
                </div>
                <div className="space-y-2">
                  <div className="text-xs font-semibold uppercase tracking-[0.28em] text-ink-muted">Obsession</div>
                  <div className="text-lg font-semibold text-ink">Сценарий первого запуска</div>
                  <div className="max-w-xs text-sm leading-6 text-ink-soft">
                    Пройдёмся по возможностям, выберем оформление и закончим на рабочем старте.
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-3 text-left text-xs text-ink-muted">
                  <div className="rounded-2xl border border-white/10 bg-white/[0.04] px-3 py-2">DPI</div>
                  <div className="rounded-2xl border border-white/10 bg-white/[0.04] px-3 py-2">Telegram</div>
                  <div className="rounded-2xl border border-white/10 bg-white/[0.04] px-3 py-2">AI</div>
                  <div className="rounded-2xl border border-white/10 bg-white/[0.04] px-3 py-2">Themes</div>
                </div>
              </div>
            </motion.div>
          </div>
        </div>
      ),
    },
    {
      id: "features",
      kicker: "Возможности",
      title: "Что умеет приложение",
      description:
        "Коротко и по делу: основные инструменты уже собраны и готовы к использованию без долгой настройки.",
      render: () => (
        <div className="space-y-6">
          <div className="grid gap-3 sm:grid-cols-2">
            {FEATURES.map((feature, index) => (
              <FeatureCard key={feature.title} feature={feature} index={index} />
            ))}
          </div>
          <div className="rounded-2xl border border-white/10 bg-white/[0.04] px-5 py-4 text-sm leading-6 text-ink-soft">
            Можно начать с любого экрана, но обычно удобнее сначала выбрать тему, а потом перейти к
            DPI-обходу или Telegram-прокси.
          </div>
        </div>
      ),
    },
    {
      id: "theme",
      kicker: "Оформление",
      title: "Выберите атмосферу",
      description:
        "Тема меняется мгновенно и в любой момент. Секретные варианты появляются только после разблокировки.",
      render: () => (
        <div className="space-y-5">
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {themeOptions.map((th, index) => {
              const selected = theme === th.id;
              return (
                <motion.button
                  key={th.id}
                  onClick={() => setTheme(th.id)}
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ ...spring.soft, delay: cascade.step * index }}
                  className={[
                    "no-drag group relative overflow-hidden rounded-2xl border p-3 text-left transition-[transform,border-color,background-color,box-shadow]",
                    selected
                      ? "border-accent/60 bg-accent/10 shadow-glow"
                      : "border-white/10 bg-white/[0.04] hover:-translate-y-0.5 hover:border-accent/25 hover:bg-white/[0.07]",
                  ].join(" ")}
                >
                  <div className="absolute inset-0 bg-gradient-to-br from-white/5 via-transparent to-accent/10 opacity-0 transition-opacity group-hover:opacity-100" />
                  <div className="relative flex flex-col items-center gap-3">
                    <div className="pointer-events-none flex h-[112px] w-[112px] items-center justify-center">
                      <ThemeMiniCore id={th.id} active={selected} />
                    </div>
                    <div className="flex w-full items-center justify-between gap-2">
                      <div className="min-w-0">
                        <div className="flex items-center gap-2 text-sm font-semibold text-ink">
                          {selected && <Icon.Check size={14} />}
                          <span>{th.label}</span>
                        </div>
                        <div className="mt-1 text-xs text-ink-muted">
                          {selected ? "Активная тема" : "Нажмите, чтобы применить"}
                        </div>
                      </div>
                      <div
                        className={[
                          "h-2.5 w-2.5 rounded-full",
                          selected ? "bg-accent shadow-glow" : "bg-white/15",
                        ].join(" ")}
                      />
                    </div>
                  </div>
                </motion.button>
              );
            })}
          </div>
          <div className="rounded-2xl border border-white/10 bg-white/[0.04] px-5 py-4 text-sm text-ink-soft">
            Тему можно поменять позже в настройках — здесь просто выберите ту атмосферу, с которой
            хотите начать.
          </div>
        </div>
      ),
    },
    {
      id: "author",
      kicker: "Финиш",
      title: "Готово к запуску",
      description:
        "Осталось одно действие: перейти в приложение и включить нужный режим в пару кликов.",
      render: () => (
        <div className="grid gap-6 lg:grid-cols-[0.9fr_1.1fr] lg:items-center">
          <div className="flex flex-col items-center gap-5 text-center lg:items-start lg:text-left">
            <motion.div
              initial={{ scale: 0.85, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={spring.soft}
              className="flex h-18 w-18 items-center justify-center rounded-3xl bg-gradient-to-br from-accent via-accent-cyan to-accent-violet shadow-glow"
            >
              <Icon.Bolt size={34} />
            </motion.div>
            <div className="space-y-2">
              <h3 className="font-display text-3xl font-semibold tracking-tight text-gradient">
                Можно начинать.
              </h3>
              <p className="max-w-md text-sm leading-6 text-ink-soft">
                Всё уже подготовлено: выберите раздел «DPI-обход», включите нужную категорию и
                продолжайте работу как обычно.
              </p>
            </div>
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            {[
              { title: "Шаг 1", text: "Выберите нужную категорию и конфиг" },
              { title: "Шаг 2", text: "Включите обход или прокси" },
              { title: "Шаг 3", text: "При необходимости смените тему" },
              { title: "Шаг 4", text: "Системa запомнит ваш выбор" },
            ].map((item, index) => (
              <motion.div
                key={item.title}
                initial={{ opacity: 0, y: 12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ ...spring.soft, delay: cascade.step * index }}
                className="rounded-2xl border border-white/10 bg-white/[0.04] px-4 py-4"
              >
                <div className="text-xs font-semibold uppercase tracking-[0.22em] text-ink-muted">
                  {item.title}
                </div>
                <div className="mt-2 text-sm leading-6 text-ink-soft">{item.text}</div>
              </motion.div>
            ))}
          </div>
        </div>
      ),
    },
  ];

  const isLast = step === steps.length - 1;
  const next = () => (isLast ? finish() : setStep((s) => s + 1));
  const back = () => setStep((s) => Math.max(0, s - 1));
  const current = steps[step];

  return (
    <AnimatePresence>
      {!closing && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: dur.base, ease: ease.enter }}
          className="fixed inset-0 z-50 flex items-center justify-center bg-base/72 px-4 backdrop-blur-md"
        >
          <motion.div
            initial={{ scale: 0.98, y: 14 }}
            animate={{ scale: 1, y: 0 }}
            exit={{ scale: 0.98, y: 10, opacity: 0 }}
            transition={spring.soft}
            className="w-[min(1120px,94vw)]"
          >
            <GlassPanel spotlight={false} className="relative overflow-hidden p-0">
              <div className="pointer-events-none absolute inset-0 bg-gradient-to-br from-accent/10 via-transparent to-accent-violet/10" />
              <div className="pointer-events-none absolute -right-20 -top-28 h-72 w-72 rounded-full bg-accent/10 blur-3xl" />
              <div className="pointer-events-none absolute -left-24 bottom-0 h-72 w-72 rounded-full bg-accent-violet/10 blur-3xl" />

              <div className="relative grid gap-0 lg:grid-cols-[1fr_1.05fr]">
                <div className="border-b border-white/10 p-6 lg:border-b-0 lg:border-r lg:p-8">
                  <StepChrome
                    kicker={current.kicker}
                    title={current.title}
                    description={current.description}
                  />
                  <div className="mt-8 hidden lg:block">
                    <div className="flex flex-col gap-2">
                      {steps.map((s, index) => {
                        const active = index === step;
                        const done = index < step;
                        return (
                          <div
                            key={s.id}
                            className={[
                              "flex items-center gap-3 rounded-2xl border px-4 py-3 transition-colors",
                              active
                                ? "border-accent/40 bg-accent/10"
                                : done
                                  ? "border-white/10 bg-white/[0.04]"
                                  : "border-white/8 bg-transparent opacity-70",
                            ].join(" ")}
                          >
                            <div
                              className={[
                                "flex h-8 w-8 items-center justify-center rounded-full text-[11px] font-semibold",
                                active
                                  ? "bg-accent text-white shadow-glow"
                                  : done
                                    ? "bg-white/10 text-ink"
                                    : "bg-white/5 text-ink-muted",
                              ].join(" ")}
                            >
                              {index + 1}
                            </div>
                            <div className="min-w-0 flex-1">
                              <div className="text-sm font-semibold text-ink">{s.title}</div>
                              <div className="truncate text-xs text-ink-muted">{s.description}</div>
                            </div>
                            {done && <Icon.Check size={14} />}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                </div>

                <div className="p-6 lg:p-8">
                  <div className="min-h-[360px]">
                    <AnimatePresence mode="wait">
                      <motion.div
                        key={current.id}
                        initial={{ opacity: 0, x: 26 }}
                        animate={{ opacity: 1, x: 0 }}
                        exit={{ opacity: 0, x: -26 }}
                        transition={{ duration: dur.base, ease: ease.enter }}
                      >
                        {current.render()}
                      </motion.div>
                    </AnimatePresence>
                  </div>

                  <div className="mt-8 flex items-center justify-between gap-4">
                    <div className="flex items-center gap-2">
                      {steps.map((_, i) => (
                        <span
                          key={i}
                          className={[
                            "h-1.5 rounded-full transition-[width,background-color]",
                            i === step ? "w-8 bg-accent" : i < step ? "w-3 bg-white/35" : "w-1.5 bg-white/14",
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

                  {!isLast && (
                    <button
                      onClick={finish}
                      className="no-drag mt-4 w-full text-center text-xs font-medium tracking-[0.08em] text-ink-muted transition-colors hover:text-ink-soft"
                    >
                      Пропустить
                    </button>
                  )}
                </div>
              </div>
            </GlassPanel>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
