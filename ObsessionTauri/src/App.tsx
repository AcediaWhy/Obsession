import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
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
import { on, win } from "./lib/tauri";
import { setWindowShown, useMotionOff } from "./design/render";
import { screenVariants } from "./design/screenTransition";
import { dur, ease, spring } from "./design/tokens";
import { toast } from "./store/toastStore";
import { useHostsStore } from "./store/hostsStore";
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
      <HeroField
        theme={theme}
        frozen={!isPresent || paused}
        phase={phase}
        screen={screen}
      />
    </motion.div>
  );
}

export default function App() {
  const [tab, setTab] = useState<Tab>("overview");
  // Направление последнего перехода по меню: +1 — вниз по списку, −1 — вверх.
  // Ref, а не state: значение нужно в том же рендере, что и смена tab.
  const tabDir = useRef(1);
  const selectTab = (next: Tab) => {
    if (next === tab) return;
    tabDir.current = TAB_ORDER.indexOf(next) > TAB_ORDER.indexOf(tab) ? 1 : -1;
    setTab(next);
  };
  const theme = useThemeStore((s) => s.theme);
  const obsessionPhase = useObsessionVisualPhase();
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const motionOff = useMotionOff();
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
    }, 650);
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
    void useHostsStore.getState().checkRoutes(900);

    // Прогрев тяжёлой ленивой сцены (Rain/WebGL) — ТОЛЬКО когда окно впервые
    // становится видимым (вызывается из резюм-ветки ниже). При старте в трее
    // (start_minimized) чанк не грузится, пока пользователь не откроет окно — не
    // держим лишний код/декодер в трее.
    let warmed = false;
    const ric = (window as unknown as {
      requestIdleCallback?: (cb: () => void) => number;
    }).requestIdleCallback;
    const scheduleWarm = () => {
      if (warmed) return;
      warmed = true;
      const warm = () => void import("./design/components/RainHybridScene");
      if (ric) ric(warm);
      else window.setTimeout(warm, 1500);
    };

    // Пауза анимаций + suspend-разгрузка при скрытии окна в трей (сигнал из Rust
    // дополняет Visibility API, который в WebView2 не всегда срабатывает на
    // hide()). Плюс гасим/возвращаем рендер веб-вью: свёрнутое (iconic) окно
    // композитор WebView2 продолжает рисовать, а hide() веб-вью убирает эту
    // нагрузку до ~0%. При ВОЗВРАТЕ догоняем backend-снимок (трей/хоткей могли
    // переключить DPI/прокси, пока висели в трее) и прогреваем сцену.
    const unlistenVis = on.windowVisibility((visible) => {
      setWindowShown(visible);
      if (visible) {
        void win.showWebview();
        void launcherBootstrap.refresh();
        void useHostsStore.getState().checkRoutes(900);
        scheduleWarm();
      } else {
        void win.hideWebview();
      }
    });
    // Мгновенный возврат рендера при развороте (фокус приходит раньше, чем
    // подтверждение 300мс-поллера) — чтобы не мелькнул пустой кадр.
    const unlistenFocus = win.onFocusChanged((focused) => {
      if (focused) void win.showWebview();
    });

    // Значимые ошибки бэкенда (падение winws, сбой Глаз/прокси) — всплывают
    // тостом, чтобы пользователь заметил их, не открывая боковой лог.
    const unlistenErr = on.log((e) => {
      if (e.level === "error") toast.error(e.message, 6000);
    });

    return () => {
      releaseBootstrap();
      unlisten.then((fn) => fn()).catch(() => {});
      unlistenVis.then((fn) => fn()).catch(() => {});
      unlistenFocus.then((fn) => fn()).catch(() => {});
      unlistenErr.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return (
    <MotionConfig reducedMotion={reduceMotion ? "always" : "user"}>
      <div
        ref={shellRef}
        data-theme={theme}
        data-reduce-motion={reduceMotion}
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
            <Parallax depth={-12} className="absolute" style={{ inset: -32 }}>
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
                        exit-пропагацию (GlassPanel, StaggerItem). Вход — пружиной
                        rise (как у каскада детей), выход — коротким duration. */}
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
}
