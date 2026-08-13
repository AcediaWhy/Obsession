import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { useSettingsStore } from "../../store/settingsStore";
import { useThemeStore, THEMES, type Theme } from "../../store/themeStore";
import { useSecretStore } from "../../store/secretStore";
import { GlassPanel } from "./GlassPanel";
import { Button } from "./atoms";
import { EyeLogo } from "./EyeLogo";
import { Icon } from "./icons";
import { CustomTitleBar } from "./CustomTitleBar";
import { ThemePreview } from "./ThemePreview";
import { spring, cascade, dur, ease } from "../tokens";

// Приветственный онбординг при первом запуске. Показывается, пока
// settings.has_completed_onboarding === false; по «Готово»/«Пропустить»
// выставляет флаг (персист в Rust). Повторно вызывается из Настроек.
//
// ВЁРСТКА. Окно приложения — 1000×680, минимум 800×600 (tauri.conf.json), а в
// Tailwind брейкпоинт `lg` = 1024px. Поэтому здесь НЕ используется ни один
// `lg:`-класс: в реальном окне он не срабатывает никогда, и вся раскладка,
// написанная под него, молча вырождалась в одну колонку с обрезанным низом.
// Опорные размеры — фиксированные и посчитаны под минимальное окно:
// панель ≤ 880px по ширине, тело шага ограничено по высоте и прокручивается.
const PANEL_WIDTH = "min(880px, 94vw)";
// 44vh от 600px = 264px; жёсткий потолок 320px — чтобы на большом окне шаг не
// расползался и оставался читаемым блоком, а не полосой во весь экран.
const BODY_MAX_HEIGHT = "min(44vh, 320px)";
const THEME_CORE_SIZE = 72;
const SECONDARY_BUTTON_CLASS = "min-h-11 focus-visible:!ring-accent-cyan";

type StepId = "intro" | "access" | "start" | "reliability" | "theme";

type StepDef = {
  id: StepId;
  kicker: string;
  title: string;
  description: string;
  render: () => JSX.Element;
};

// ─── Мелкие строительные блоки ───────────────────────────────────────────────

function InfoCard({
  icon: IconCmp,
  title,
  children,
  index = 0,
  tone = "accent",
}: {
  icon: (p: { size?: number }) => JSX.Element;
  title: string;
  children: React.ReactNode;
  index?: number;
  tone?: "accent" | "warn";
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ ...spring.soft, delay: cascade.step * index }}
      className="rounded-xl border border-white/[0.07] bg-base-800/40 p-3.5"
    >
      <div className="flex gap-3">
        <div
          className={[
            "mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg",
            tone === "warn" ? "bg-warn/10 text-warn" : "bg-accent/15 text-accent-cyan",
          ].join(" ")}
        >
          <IconCmp size={17} />
        </div>
        <div className="min-w-0">
          <div className="text-sm font-semibold text-ink">{title}</div>
          <div className="mt-1 text-xs leading-5 text-ink-soft">{children}</div>
        </div>
      </div>
    </motion.div>
  );
}

// Строка нумерованного сценария («как включить»): цифра + текст.
function NumberedRow({
  n,
  title,
  children,
}: {
  n: number;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ ...spring.soft, delay: cascade.step * (n - 1) }}
      className="flex gap-3 rounded-xl border border-white/[0.07] bg-base-800/40 p-3.5"
    >
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-accent/15 text-xs font-semibold text-accent-cyan">
        {n}
      </div>
      <div className="min-w-0">
        <div className="text-sm font-semibold text-ink">{title}</div>
        <div className="mt-1 text-xs leading-5 text-ink-soft">{children}</div>
      </div>
    </motion.div>
  );
}

function ThemeMiniCore({ id, active }: { id: Theme; active: boolean }) {
  return <ThemePreview theme={id} selected={active} size={THEME_CORE_SIZE} />;
}

export function getOnboardingKeyboardIntent(
  key: string,
  confirmationOpen: boolean,
  saving: boolean,
): "open_confirmation" | "close_confirmation" | null {
  if (key !== "Escape" || saving) return null;
  return confirmationOpen ? "close_confirmation" : "open_confirmation";
}

// Accent у светлых тем требует тёмного foreground, у Aurora/Fallen — белого.
// Держим hover на 90% opacity: full accent в этих двух темах граничит с 4.5:1.
export function getOnboardingPrimaryButtonClass(theme: Theme): string {
  return [
    "min-h-11 hover:!bg-accent/90 focus-visible:!ring-accent-cyan",
    theme === "aurora" || theme === "fallendown" ? "!text-white" : "!text-[#05060B]",
  ].join(" ");
}

// ─── Компонент ───────────────────────────────────────────────────────────────

export function Onboarding() {
  const patchConfirmed = useSettingsStore((s) => s.patchConfirmed);
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const unlocked = useSecretStore((s) => s.unlocked);

  const [step, setStep] = useState(0);
  const [confirmSkip, setConfirmSkip] = useState(false);
  const [saving, setSaving] = useState(false);
  const [completionError, setCompletionError] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const skipButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const confirmationReturnFocusRef = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();
  const confirmationTitleId = useId();
  const confirmationDescriptionId = useId();

  const themeOptions = useMemo(
    () => THEMES.filter((th) => !th.secret || unlocked.includes(th.secret)),
    [unlocked],
  );
  const primaryButtonClass = getOnboardingPrimaryButtonClass(theme);

  const finish = useCallback(async () => {
    if (saving) return;
    setSaving(true);
    setCompletionError("");
    const saved = await patchConfirmed({ has_completed_onboarding: true });
    if (!saved) {
      setSaving(false);
      setCompletionError(
        "Не удалось сохранить завершение. Проверь доступ к данным приложения и повтори попытку.",
      );
    }
    // При успехе authoritative store update размонтирует onboarding. Не меняем
    // локальный state после await, чтобы не обновлять уже размонтированный dialog.
  }, [patchConfirmed, saving]);

  const requestSkip = useCallback(() => {
    if (saving) return;
    confirmationReturnFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setCompletionError("");
    setConfirmSkip(true);
  }, [saving]);

  const cancelSkip = useCallback(() => {
    if (saving) return;
    setCompletionError("");
    setConfirmSkip(false);
    window.setTimeout(() => {
      if (confirmationReturnFocusRef.current?.isConnected) {
        confirmationReturnFocusRef.current.focus();
      } else if (skipButtonRef.current) {
        skipButtonRef.current.focus();
      } else {
        headingRef.current?.focus();
      }
    }, 0);
  }, [saving]);

  const steps: StepDef[] = [
    {
      id: "intro",
      kicker: "Первый запуск",
      title: "Три инструмента в одном окне",
      description:
        "Obsession теперь запускается без постоянного UAC. Системные функции временно приостановлены до установки защищённого компонента.",
      render: () => (
        <div className="grid gap-3 sm:grid-cols-3">
          <InfoCard icon={Icon.Bolt} title="DPI-обход" index={0}>
            Готовые конфиги Zapret для Discord, YouTube&nbsp;/&nbsp;Twitch, игр и универсальный
            набор. Выбираешь категорию — приложение поднимает winws.
          </InfoCard>
          <InfoCard icon={Icon.Robot} title="Доступ к ИИ" index={1}>
            ChatGPT, Claude, Gemini и другие — через системный{" "}
            <code className="font-mono">hosts</code>. Запись атомарная, с бэкапом: снимается одной
            кнопкой.
          </InfoCard>
          <InfoCard icon={Icon.Send} title="Telegram-прокси" index={2}>
            MTProto через WebSocket в один клик. Ссылка{" "}
            <code className="font-mono">tg://proxy</code> и QR-код, чтобы подключить телефон из той
            же сети.
          </InfoCard>
        </div>
      ),
    },
    {
      id: "access",
      kicker: "Доступ",
      title: "Безопасная граница привилегий",
      description:
        "Обычный интерфейс больше не запрашивает права администратора. DPI, hosts и запуск сетевых утилит вернутся через отдельный защищённый runtime.",
      render: () => (
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-2">
            <InfoCard icon={Icon.Shield} title="Драйвер WinDivert" index={0}>
              Драйвер и winws требуют системных прав. До появления подписанного helper/service
              приложение намеренно не запускает их из пользовательской папки.
            </InfoCard>
            <InfoCard icon={Icon.Settings} title="Системный hosts" index={1}>
              Файл <code className="font-mono">System32\drivers\etc\hosts</code> пока доступен
              только для проверки. Запись и откат будут выполняться защищённым компонентом.
            </InfoCard>
          </div>
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ ...spring.soft, delay: cascade.step * 2 }}
            className="flex gap-3 rounded-xl border border-white/[0.07] bg-base-800/30 px-4 py-3"
          >
            <div className="mt-0.5 shrink-0 text-ink-soft">
              <Icon.Info size={16} />
            </div>
            <p className="text-xs leading-5 text-ink-soft">
              Настройки и кэши по-прежнему лежат локально в{" "}
              <code className="font-mono">%APPDATA%\Obsession</code>, но EXE, DLL, драйверы,
              Lua и системные снапшоты больше не считаются доверенными из этой папки.
            </p>
          </motion.div>
        </div>
      ),
    },
    {
      id: "start",
      kicker: "Старт",
      title: "Как включить обход",
      description:
        "Категории и профили можно настроить заранее. Запуск станет доступен после установки защищённого runtime.",
      render: () => (
        <div className="space-y-3">
          <NumberedRow n={1} title="Выбери категорию">
            Discord, YouTube&nbsp;/&nbsp;Twitch, Gaming, Universal или «Под угрозой». Категория
            определяет, к каким сервисам применяется обход.
          </NumberedRow>
          <NumberedRow n={2} title="Проверь конфиг">
            Можно оставить предложенный. Кнопка «Тест» честно дёргает заблокированный ресурс и
            показывает, пробивает ли конкретный конфиг блокировку.
          </NumberedRow>
          <NumberedRow n={3} title="Включи защиту">
            После безопасной миграции системный компонент поднимет winws из защищённой папки,
            а интерфейс останется обычным процессом без постоянного UAC.
          </NumberedRow>
        </div>
      ),
    },
    {
      id: "reliability",
      kicker: "Надёжность",
      title: "Обход умеет замечать, что его режут",
      description:
        "ТСПУ подстраиваются, поэтому обход замкнут в петлю наблюдения. Отсюда и название приложения.",
      render: () => (
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-3">
            <InfoCard icon={Icon.Eye} title="Глаза" index={0}>
              Слушают копию собственного трафика и различают, что случилось: соединение сбросили или
              его тихо роняют.
            </InfoCard>
            <InfoCard icon={Icon.Globe} title="Память сети" index={1}>
              Запоминает, что работало именно в этой сети — по шлюзу и региону провайдера. В знакомой
              сети рабочий конфиг поднимается сразу.
            </InfoCard>
            <InfoCard icon={Icon.Refresh} title="Мозг" index={2}>
              Меняет стратегию строгой лестницей с запасом — аккуратно, чтобы перебором не
              спровоцировать блокировку жёстче прежней.
            </InfoCard>
          </div>
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ ...spring.soft, delay: cascade.step * 3 }}
            className="flex gap-3 rounded-xl border border-white/[0.07] bg-base-800/30 px-4 py-3"
          >
            <div className="mt-0.5 shrink-0 text-ink-soft">
              <Icon.Info size={16} />
            </div>
            <p className="text-xs leading-5 text-ink-soft">
              По умолчанию контур в режиме «Наблюдение»: он только показывает, что происходит, и сам
              ничего не переключает. Право действовать выдаётся отдельно — режимом «С подтверждением»
              или «Автоматический» в разделе надёжности.
            </p>
          </motion.div>
        </div>
      ),
    },
    {
      id: "theme",
      kicker: "Финиш",
      title: "Выбери атмосферу",
      description:
        "Тему можно сменить в любой момент в Настройках. Скрытые появляются после разблокировки.",
      render: () => (
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
          {themeOptions.map((th, index) => {
            const selected = theme === th.id;
            return (
              <motion.button
                key={th.id}
                type="button"
                onClick={() => setTheme(th.id)}
                aria-pressed={selected}
                aria-label={`Тема «${th.label}»${selected ? ", выбрана" : ""}`}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ ...spring.soft, delay: cascade.step * index }}
                className={[
                  "no-drag group relative flex flex-col items-center gap-2 rounded-xl border p-2.5 transition-[border-color,background-color,box-shadow]",
                  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-cyan",
                  selected
                    ? "border-accent/50 bg-accent/10 shadow-glow"
                    : "border-white/[0.07] bg-base-800/40 hover:border-accent/25 hover:bg-base-800/60",
                ].join(" ")}
              >
                <div
                  className="pointer-events-none flex items-center justify-center"
                  style={{ width: THEME_CORE_SIZE, height: THEME_CORE_SIZE }}
                >
                  <ThemeMiniCore id={th.id} active={selected} />
                </div>
                <div className="flex items-center gap-1.5">
                  {selected && <Icon.Check size={13} />}
                  <span
                    className={`text-xs font-semibold ${selected ? "text-ink" : "text-ink-soft"}`}
                  >
                    {th.label}
                  </span>
                </div>
              </motion.button>
            );
          })}
        </div>
      ),
    },
  ];

  const isLast = step === steps.length - 1;
  const next = () => {
    setCompletionError("");
    if (isLast) void finish();
    else setStep((s) => Math.min(steps.length - 1, s + 1));
  };
  const back = () => {
    setCompletionError("");
    setStep((s) => Math.max(0, s - 1));
  };
  const current = steps[step];

  // Восстанавливаем focus после повторного запуска из Настроек. Таймер даёт App
  // сначала снять inert с основного shell после размонтирования modal.
  useEffect(() => {
    previousFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      const previous = previousFocusRef.current;
      window.setTimeout(() => {
        if (previous?.isConnected) previous.focus();
      }, 0);
    };
  }, []);

  // Смена шага объявляется переносом focus на новый heading. Одновременно
  // возвращаем прокрутку тела наверх, чтобы следующий шаг не открылся с середины.
  useEffect(() => {
    bodyRef.current?.scrollTo({ top: 0 });
    headingRef.current?.focus();
  }, [step, confirmSkip]);

  // Modal keyboard contract: только focus trap и безопасный Escape. Enter и
  // стрелки остаются нативным controls/scroll — глобальная навигация ломала Back
  // и theme tiles.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        const intent = getOnboardingKeyboardIntent(event.key, confirmSkip, saving);
        if (!intent) return;
        event.preventDefault();
        if (intent === "open_confirmation") requestSkip();
        else cancelSkip();
        return;
      }
      if (event.key !== "Tab") return;

      const root = dialogRef.current;
      if (!root) return;
      const focusable = Array.from(
        root.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter(
        (element) =>
          !element.hasAttribute("hidden") && element.getAttribute("aria-hidden") !== "true",
      );

      if (focusable.length === 0) {
        event.preventDefault();
        headingRef.current?.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !root.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [cancelSkip, confirmSkip, requestSkip, saving]);

  return (
    <motion.div
      ref={dialogRef}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: dur.base, ease: ease.enter }}
      role="dialog"
      aria-modal="true"
      aria-labelledby={confirmSkip ? confirmationTitleId : titleId}
      aria-describedby={confirmSkip ? confirmationDescriptionId : descriptionId}
      aria-busy={saving}
      className="fixed inset-0 z-50 flex flex-col bg-base/[0.72] backdrop-blur-md"
    >
      {/* Оригинальный titlebar находится в inert shell. Копия внутри dialog
          сохраняет доступ к drag/minimize/maximize/close, не выпуская focus наружу. */}
      <div className="relative z-10 shrink-0">
        <CustomTitleBar />
      </div>

      <div className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-4 py-3">
        <motion.div
          initial={{ scale: 0.98, y: 12 }}
          animate={{ scale: 1, y: 0 }}
          transition={spring.soft}
          style={{ width: PANEL_WIDTH }}
          className="my-auto"
        >
          {/* padded={false} обязателен: свой `p-5` у GlassPanel и наш внутренний
              давали двойной отступ, а `p-0` в className его не перебивал. */}
          <GlassPanel spotlight={false} padded={false} className="relative">
            {confirmSkip ? (
              <div className="p-6">
                <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-ink-soft">
                  <span className="h-1.5 w-1.5 rounded-full bg-warn" />
                  Первый запуск
                </div>
                <h2
                  ref={headingRef}
                  id={confirmationTitleId}
                  tabIndex={-1}
                  className="mt-4 font-display text-2xl font-semibold tracking-tight text-ink outline-none"
                >
                  Пропустить знакомство?
                </h2>
                <p
                  id={confirmationDescriptionId}
                  className="mt-2 max-w-xl text-sm leading-6 text-ink-soft"
                >
                  Системные настройки не изменятся. Выбранная тема останется, а знакомство можно в
                  любой момент открыть снова в разделе «Настройки».
                </p>
                {completionError && (
                  <p
                    role="alert"
                    className="mt-4 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-ink"
                  >
                    {completionError}
                  </p>
                )}
                <div className="mt-6 flex flex-wrap justify-end gap-2">
                  <Button
                    variant="ghost"
                    onClick={cancelSkip}
                    disabled={saving}
                    className={SECONDARY_BUTTON_CLASS}
                  >
                    Вернуться
                  </Button>
                  <Button
                    variant="primary"
                    onClick={() => void finish()}
                    disabled={saving}
                    className={primaryButtonClass}
                  >
                    {saving ? "Сохраняем…" : "Пропустить"}
                  </Button>
                </div>
              </div>
            ) : (
              <div className="p-5">
                {/* Шапка: логотип, счётчик и доступный сегментированный прогресс. */}
                <div className="flex items-center justify-between gap-4">
                  <div className="flex items-center gap-2.5">
                    <EyeLogo size={26} />
                    <span className="font-display text-sm font-semibold tracking-tight text-ink">
                      Obsession
                    </span>
                  </div>
                  <span className="font-mono text-xs text-ink-soft">
                    Шаг {step + 1} из {steps.length}
                  </span>
                </div>

                <div
                  className="mt-3 flex gap-1.5"
                  role="progressbar"
                  aria-label="Прогресс знакомства"
                  aria-valuemin={1}
                  aria-valuemax={steps.length}
                  aria-valuenow={step + 1}
                  aria-valuetext={`Шаг ${step + 1} из ${steps.length}: ${current.title}`}
                >
                  {steps.map((s, i) => (
                    <span
                      key={s.id}
                      aria-hidden="true"
                      className={[
                        "h-1 flex-1 rounded-full transition-colors duration-300",
                        i < step ? "bg-accent/45" : i === step ? "bg-accent" : "bg-white/10",
                      ].join(" ")}
                    />
                  ))}
                </div>

                <div className="mt-4 space-y-1.5">
                  <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-xs font-semibold uppercase tracking-[0.18em] text-ink-soft">
                    <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-glow" />
                    {current.kicker}
                  </div>
                  <h2
                    ref={headingRef}
                    id={titleId}
                    tabIndex={-1}
                    className="font-display text-xl font-semibold tracking-tight text-ink outline-none"
                  >
                    {current.title}
                  </h2>
                  <p id={descriptionId} className="text-xs leading-5 text-ink-soft">
                    {current.description}
                  </p>
                </div>

                <div
                  ref={bodyRef}
                  tabIndex={0}
                  role="region"
                  aria-labelledby={titleId}
                  className="scroll-fade mt-2 overflow-y-auto py-4 pr-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-cyan"
                  style={{ maxHeight: BODY_MAX_HEIGHT }}
                >
                  <AnimatePresence mode="wait">
                    <motion.div
                      key={current.id}
                      initial={{ opacity: 0, x: 18 }}
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0, x: -18 }}
                      transition={{ duration: dur.fast, ease: ease.enter }}
                    >
                      {current.render()}
                    </motion.div>
                  </AnimatePresence>
                </div>

                {completionError && (
                  <p
                    role="alert"
                    className="mt-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-ink"
                  >
                    {completionError}
                  </p>
                )}

                <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
                  {isLast ? (
                    <span />
                  ) : (
                    <button
                      ref={skipButtonRef}
                      type="button"
                      onClick={requestSkip}
                      disabled={saving}
                      className="no-drag min-h-11 rounded-xl px-3 text-xs font-semibold tracking-[0.04em] text-ink-soft transition-colors hover:bg-white/5 hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-cyan disabled:opacity-50"
                    >
                      Пропустить
                    </button>
                  )}
                  <div className="flex flex-wrap items-center justify-end gap-2">
                    {step > 0 && (
                      <Button
                        variant="ghost"
                        onClick={back}
                        disabled={saving}
                        className={SECONDARY_BUTTON_CLASS}
                      >
                        Назад
                      </Button>
                    )}
                    <Button
                      variant="primary"
                      onClick={next}
                      disabled={saving}
                      className={primaryButtonClass}
                    >
                      {saving ? "Сохраняем…" : isLast ? "Начать" : "Далее"}
                    </Button>
                  </div>
                </div>
              </div>
            )}
          </GlassPanel>
        </motion.div>
      </div>
    </motion.div>
  );
}
