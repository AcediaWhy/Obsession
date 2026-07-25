import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  AnimatePresence,
  motion,
  MotionConfig,
  useIsPresent,
  type Variants,
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
import { Onboarding } from "./design/components/Onboarding";
import { Toaster } from "./design/components/Toaster";
import { on, win } from "./lib/tauri";
import { setWindowShown, useMotionOff } from "./design/render";
import { dur, ease, spring } from "./design/tokens";
import { toast } from "./store/toastStore";

// Направленный слайд экранов: контент движется в сторону перехода по меню
// (вниз по списку — уходит вверх, вверх — вниз). Направление приходит через
// custom: у уходящего экрана пропсы заморожены AnimatePresence, и только
// custom на самом AnimatePresence обновляется для его exit-варианта.
// Обёртка transform-only (без opacity) — см. комментарий у <motion.div key={tab}>.
const screenVariants: Variants = {
  // 16px хода: под мягкую пружину rise меньший путь почти не читается.
  enter: (dir: number) => ({ y: 16 * dir }),
  center: { y: 0 },
  exit: (dir: number) => ({
    y: -12 * dir,
    transition: { duration: dur.fast, ease: ease.exit },
  }),
};

function ThemeScene({ theme, motionOff }: { theme: Theme; motionOff: boolean }) {
  const isPresent = useIsPresent();
  return (
    <motion.div
      className="absolute inset-0"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: motionOff ? 0 : dur.slow, ease: ease.xfade }}
    >
      <HeroField theme={theme} frozen={!isPresent} />
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
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const motionOff = useMotionOff();
  const showOnboarding = useSettingsStore(
    (s) => s.loaded && !!s.settings && !s.settings.has_completed_onboarding,
  );

  // Морф темы активируется DOM-атрибутом до paint и не создаёт два лишних
  // React-render на каждое переключение. CSS ограничивает переходы семантическими
  // поверхностями/контролами вместо universal selector по всему дереву.
  const shellRef = useRef<HTMLDivElement>(null);
  const prevTheme = useRef(theme);
  useLayoutEffect(() => {
    const shell = shellRef.current;
    if (!shell) return;
    if (prevTheme.current === theme) {
      if (motionOff) delete shell.dataset.themeMorph;
      return;
    }
    prevTheme.current = theme;
    if (motionOff) {
      delete shell.dataset.themeMorph;
      return;
    }

    shell.dataset.themeMorph = "true";
    const timer = window.setTimeout(() => {
      delete shell.dataset.themeMorph;
    }, 650);
    return () => {
      window.clearTimeout(timer);
      delete shell.dataset.themeMorph;
    };
  }, [theme, motionOff]);

  // Инициализация сторов и подписок — один раз при старте.
  useEffect(() => {
    const unlisten = initLogStream();
    const releaseBootstrap = launcherBootstrap.acquire();

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
      >
        <ParallaxProvider>
          {/* Дальний план: фон движется против курсора. Оверскан по краям, чтобы
              сдвиг никогда не оголял углы. Смена темы — кроссфейд: обе сцены
              живут ~0.42 с (тема уходящей ветки заморожена пропом, см.
              HeroField), поверх старой проявляется новая. На холодном старте
              initial-фейд даёт мягкое появление фона. */}
          <Parallax depth={-12} className="absolute" style={{ inset: -32 }}>
            <AnimatePresence mode="sync">
              <ThemeScene key={theme} theme={theme} motionOff={motionOff} />
            </AnimatePresence>
          </Parallax>

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

          {/* Онбординг первого запуска — поверх всего, пока флаг не выставлен. */}
          {showOnboarding && <Onboarding />}

          {/* Тосты — единый канал коротких сообщений поверх всего. */}
          <Toaster />
        </ParallaxProvider>
      </div>
    </MotionConfig>
  );
}
