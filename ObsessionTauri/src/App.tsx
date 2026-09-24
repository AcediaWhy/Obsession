import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactElement } from "react";
import {
  AnimatePresence,
  motion,
  MotionConfig,
  useIsPresent,
} from "framer-motion";

import { HeroField } from "./design/components/HeroField";
import { ParallaxProvider, Parallax } from "./design/parallax";
import { CustomTitleBar } from "./design/components/CustomTitleBar";
import { NavRail, TAB_ORDER, type Tab } from "./design/components/NavRail";
import { OverviewScreen } from "./screens/Overview";
import { DpiScreen } from "./screens/Dpi";
import { AiScreen } from "./screens/Ai";
import { TelegramScreen } from "./screens/Telegram";
import { ListsScreen } from "./screens/Lists";
import { SettingsScreen } from "./screens/Settings";
import { ProfilesScreen } from "./screens/Profiles";

import { initLogStream } from "./store/logStore";
import { launcherBootstrap } from "./store/launcherBootstrap";
import { useThemeStore, type Theme } from "./store/themeStore";
import { useSettingsStore } from "./store/settingsStore";
import { useOnboardingStore } from "./store/onboardingStore";
import { Onboarding } from "./design/components/Onboarding";
import { Toaster } from "./design/components/Toaster";
import { on } from "./lib/tauri";
import { setWindowShown, useMotionOff, useRenderHidden } from "./design/render";
import { screenVariants } from "./design/screenTransition";
import { initTrayStageRelease } from "./design/gl/trayStageRelease";
import { initTraySleep } from "./design/traySleep";
import { dur, ease, spring } from "./design/tokens";
import { toast } from "./store/toastStore";
import { useObsessionVisualPhase } from "./design/useObsessionVisualPhase";
import { obsessionFocusForScreen, type ObsessionVisualPhase } from "./design/obsessionVisualState";

function ThemeScene({
  theme,
  motionOff,
  paused,
  phase,
  screen,
}: {
  theme: Theme;
  motionOff: boolean;
  paused: boolean;
  phase: ObsessionVisualPhase;
  screen: Tab;
}) {
  const isPresent = useIsPresent();
  return (
    <motion.div
      className="absolute inset-0"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: motionOff ? 0 : dur.slow, ease: ease.xfade }}
    >
      {isPresent ? (
        <HeroField
          theme={theme}
          frozen={paused}
          phase={phase}
          screen={screen}
        />
      ) : (
        // Exit-обёртка остаётся для плавного fade, но тяжёлая сцена (canvas,
        // WebGL/video и её compositor-слои) должна уйти сразу при начале exit.
        <div
          aria-hidden="true"
          data-theme-scene-exit-placeholder
          className="absolute inset-0"
        />
      )}
    </motion.div>
  );
}

export default function App() {
  const [tab, setTab] = useState<Tab>("overview");
  // Направление последнего перехода по меню: +1 — вниз по списку, −1 — вверх.
  // Ref, а не state: значение нужно в том же рендере, что и смена tab.
  const tabDir = useRef(1);
  const theme = useThemeStore((s) => s.theme);
  const obsessionPhase = useObsessionVisualPhase();
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const motionOff = useMotionOff();
  const selectTab = (next: Tab) => {
    if (next === tab) return;
    tabDir.current = TAB_ORDER.indexOf(next) > TAB_ORDER.indexOf(tab) ? 1 : -1;
    setTab(next);
  };
  // Заморозка скрытого окна: пока окно в трее, рендер отдаёт предыдущее
  // дерево как есть — React видит тот же элемент и пропускает согласование
  // поддерева целиком (см. кэш перед return ниже).
  const hidden = useRenderHidden();
  const hiddenCache = useRef<ReactElement | null>(null);
  const onboardingSnapshot = useOnboardingStore((s) => s.snapshot);
  const onboardingLoaded = useOnboardingStore((s) => s.loaded);
  const initializeOnboarding = useOnboardingStore((s) => s.initialize);
  const startOnboarding = useOnboardingStore((s) => s.start);
  const skipOnboarding = useOnboardingStore((s) => s.skip);
  const showOnboarding =
    onboardingLoaded &&
    (onboardingSnapshot?.presentation === "required" ||
      onboardingSnapshot?.presentation === "modal");
  const showOnboardingOffer =
    onboardingLoaded && onboardingSnapshot?.presentation === "offer";

  useEffect(() => {
    if (settingsLoaded) void initializeOnboarding();
  }, [initializeOnboarding, settingsLoaded]);

  // Морф темы активируется DOM-атрибутом до paint и не создаёт два лишних
  // React-render на каждое переключение. CSS ограничивает переходы семантическими
  // поверхностями/контролами вместо universal selector по всему дереву.
  const shellRef = useRef<HTMLDivElement>(null);
  const interactionShellRef = useRef<HTMLDivElement>(null);
  const prevTheme = useRef(theme);
  const [focusCapture, setFocusCapture] = useState(false);
  useLayoutEffect(() => {
    const shell = shellRef.current;
    if (!shell) return;
    if (prevTheme.current === theme) {
      if (motionOff) {
        delete shell.dataset.themeMorph;
        setFocusCapture(false);
      }
      return;
    }
    const enteringObsession = theme === "obsession";
    prevTheme.current = theme;
    if (motionOff) {
      delete shell.dataset.themeMorph;
      setFocusCapture(false);
      return;
    }

    shell.dataset.themeMorph = "true";
    if (enteringObsession) setFocusCapture(true);
    const timer = window.setTimeout(() => {
      delete shell.dataset.themeMorph;
      setFocusCapture(false);
    }, dur.slow * 1000);
    return () => {
      window.clearTimeout(timer);
      delete shell.dataset.themeMorph;
      setFocusCapture(false);
    };
  }, [theme, motionOff]);

  const obsessionFocus = obsessionFocusForScreen(tab);
  const obsessionStyle = {
    "--obsession-focus-x": `${obsessionFocus.x * 100}%`,
    "--obsession-focus-y": `${obsessionFocus.y * 100}%`,
  } as CSSProperties;

  // Modal onboarding — единственная интерактивная ветка. Inert убирает основной
  // shell из tab/accessibility tree; атрибут ставим напрямую для совместимости
  // с текущими React typings WebView2.
  useLayoutEffect(() => {
    const shell = interactionShellRef.current;
    if (!shell) return;
    if (showOnboarding) {
      shell.setAttribute("inert", "");
      shell.setAttribute("aria-hidden", "true");
    } else {
      shell.removeAttribute("inert");
      shell.removeAttribute("aria-hidden");
    }
  }, [showOnboarding]);

  // Инициализация сторов и подписок — один раз при старте.
  useEffect(() => {
    const unlisten = initLogStream();
    const releaseBootstrap = launcherBootstrap.acquire();
    // Трей-выгрузка WebGL-стейджей: секунда скрытого окна — стейджи уходят
    // из памяти, возврат пересобирает сцену один раз.
    const releaseTrayStages = initTrayStageRelease();
    const releaseTraySleep = initTraySleep();
    let disposed = false;
    let bootstrapRefreshQueue = Promise.resolve();
    const scheduleBootstrapRefresh = (refreshSnapshot: boolean) => {
      bootstrapRefreshQueue = bootstrapRefreshQueue.then(async () => {
        if (disposed) return;
        if (refreshSnapshot) await launcherBootstrap.refresh();
        else await launcherBootstrap.whenReady();
      });
    };
    // Не запускаем здесь AI route probe: защищённая служба обслуживает один
    // pipe, а сетевой hosts-check может удерживать его до 25 секунд и ставить
    // пользовательский DPI/proxy start в очередь. Проверка остаётся доступна
    // явно на экране ИИ.
    scheduleBootstrapRefresh(false);

    // Сигнал Rust дополняет Visibility API, который в WebView2 может пропустить
    // скрытие окна. При возвращении обновляем состояние DPI и прокси: за время
    // работы в трее они могли измениться через трей или горячую клавишу.
    // WebView здесь не скрываем: асинхронные hide()/show() могут завершиться
    // в обратном порядке и оставить видимое окно с чёрным фоном.
    const unlistenVis = on.windowVisibility((visible) => {
      setWindowShown(visible);
      if (visible) {
        scheduleBootstrapRefresh(true);
      }
    });

    // Значимые ошибки бэкенда (падение winws, сбой Глаз/прокси) — всплывают
    // тостом, чтобы пользователь заметил их, не открывая боковой лог.
    const unlistenErr = on.log((e) => {
      if (e.level === "error") toast.error(e.message, 6000);
    });

    return () => {
      disposed = true;
      releaseBootstrap();
      releaseTrayStages();
      releaseTraySleep();
      unlisten.then((fn) => fn()).catch(() => {});
      unlistenVis.then((fn) => fn()).catch(() => {});
      unlistenErr.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // Пока окно скрыто — отдаём закэшированное дерево: идентичная ссылка
  // элемента заставляет React пропустить согласование всего поддерева.
  // Невидимый UI перестаёт перерисовывать поток событий живой сессии
  // (фазы Eyes, статусы, логи); сторы копят состояние, на показе дерево
  // строится заново одним рендером и догоняет актуальное.
  if (hidden && hiddenCache.current) {
    return hiddenCache.current;
  }
  const tree = (
    <MotionConfig reducedMotion={reduceMotion ? "always" : "user"}>
      {/* Единый effective-флаг: настройка приложения ИЛИ системный
          prefers-reduced-motion. Иначе CSS продолжал animate-pulse/transition,
          хотя Framer и canvas уже останавливались через useMotionOff(). */}
      <div
        ref={shellRef}
        data-theme={theme}
        data-reduce-motion={motionOff}
        className="relative h-screen w-screen overflow-hidden"
        style={obsessionStyle}
      >
        <ParallaxProvider paused={showOnboarding}>
          <div
            ref={interactionShellRef}
            data-app-shell
            aria-hidden={showOnboarding || undefined}
            className={`absolute inset-0 ${showOnboarding ? "pointer-events-none" : ""}`}
          >
            {/* Дальний план: фон движется против курсора. Оверскан по краям, чтобы
                сдвиг никогда не оголял углы. Смена темы — кроссфейд: обе сцены
                живут ~0.42 с (тема уходящей ветки заморожена пропом, см.
                HeroField), поверх старой проявляется новая. На холодном старте
                initial-фейд даёт мягкое появление фона. */}
            <Parallax
              depth={theme === "goldenmeadow" || theme === "ophanim" ? 0 : -12}
              className="absolute"
              style={{ inset: theme === "goldenmeadow" || theme === "ophanim" ? 0 : -32 }}
            >
              <AnimatePresence mode="sync">
                <ThemeScene
                  key={theme}
                  theme={theme}
                  motionOff={motionOff}
                  paused={showOnboarding}
                  phase={obsessionPhase}
                  screen={tab}
                />
              </AnimatePresence>
            </Parallax>

            <div
              aria-hidden="true"
              data-active={focusCapture || undefined}
              className="obsession-focus-capture pointer-events-none absolute inset-0 z-[1]"
            />

            {/* Титлбар — чистый хром, без параллакса. */}
            <div className="absolute inset-x-0 top-0 z-20">
              <CustomTitleBar />
            </div>

            {/* Контент. */}
            <div className="absolute inset-0 top-10 flex">
              <NavRail active={tab} onSelect={selectTab} />
              <main className="flex-1 overflow-hidden px-6 pb-6 pt-2">
                <div className="relative h-full">
                  <AnimatePresence mode="sync" custom={tabDir.current}>
                    {/* Обёртка экрана — transform-only: opacity у предка стекла
                        образует backdrop root (Chromium), и панели теряли матовость
                        на время перехода. Фейд делают сами панели/элементы через
                        exit-пропагацию (GlassPanel, StaggerItem). mode="sync"
                        гарантирует, что новый экран смонтируется даже если exit
                        предыдущей вкладки был прерван быстрым переключением. */}
                    <motion.div
                      key={tab}
                      custom={tabDir.current}
                      variants={screenVariants}
                      initial="enter"
                      animate="center"
                      exit="exit"
                      transition={spring.rise}
                      className="absolute inset-0 h-full"
                    >
                      {tab === "overview" && <OverviewScreen />}
                      {tab === "dpi" && <DpiScreen />}
                      {tab === "ai" && <AiScreen />}
                      {tab === "telegram" && <TelegramScreen />}
                      {tab === "lists" && <ListsScreen />}
                      {tab === "profiles" && <ProfilesScreen />}
                      {tab === "settings" && <SettingsScreen />}
                    </motion.div>
                  </AnimatePresence>
                </div>
              </main>
            </div>

            {/* Тосты основного приложения принадлежат inert shell. Ошибки
                завершения onboarding показываются внутри самого dialog. */}
            <Toaster />

            {showOnboardingOffer && (
              <aside className="no-drag absolute bottom-5 right-5 z-40 w-[min(390px,calc(100vw-40px))] rounded-2xl border border-white/[0.1] bg-base-900/95 p-4 shadow-2xl backdrop-blur-xl" aria-label="Предложение настройки">
                <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-accent-cyan">Onboarding V2</p>
                <h2 className="mt-1.5 text-sm font-semibold text-ink">Настроить функции через защищённую службу?</h2>
                <p className="mt-1 text-xs leading-5 text-ink-soft">Для существующей установки это необязательное предложение. Текущая конфигурация сохранится до явного Review.</p>
                <div className="mt-3 flex justify-end gap-2">
                  <button type="button" onClick={() => void skipOnboarding()} className="rounded-xl border border-white/[0.08] bg-white/5 px-3.5 py-2 text-xs font-medium text-ink-soft hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent">Не сейчас</button>
                  <button type="button" onClick={() => void startOnboarding("soft_offer")} className="rounded-xl bg-accent/90 px-3.5 py-2 text-xs font-semibold text-white hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-cyan">Настроить сейчас</button>
                </div>
              </aside>
            )}
          </div>

          {/* Онбординг первого запуска — единственная активная modal-ветка. */}
          {showOnboarding && (
            <Onboarding
              onDestination={(destination) => {
                selectTab(destination);
                void launcherBootstrap.refresh();
              }}
            />
          )}
        </ParallaxProvider>
      </div>
    </MotionConfig>
  );
  hiddenCache.current = tree;
  return tree;
}
