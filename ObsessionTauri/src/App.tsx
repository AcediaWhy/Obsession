import { useEffect, useState } from "react";
import { AnimatePresence, motion, MotionConfig } from "framer-motion";

import { HeroField } from "./design/components/HeroField";
import { ParallaxProvider, Parallax } from "./design/parallax";
import { CustomTitleBar } from "./design/components/CustomTitleBar";
import { NavRail, type Tab } from "./design/components/NavRail";
import { OverviewScreen } from "./screens/Overview";
import { DpiScreen } from "./screens/Dpi";
import { AiScreen } from "./screens/Ai";
import { TelegramScreen } from "./screens/Telegram";
import { ListsScreen } from "./screens/Lists";
import { SettingsScreen } from "./screens/Settings";
import { ProfilesScreen } from "./screens/Profiles";

import { initLogStream } from "./store/logStore";
import { useDpiStore } from "./store/dpiStore";
import { useProxyStore } from "./store/proxyStore";
import { useHostsStore } from "./store/hostsStore";
import { useThemeStore } from "./store/themeStore";
import { useSettingsStore } from "./store/settingsStore";
import { Onboarding } from "./design/components/Onboarding";
import { Toaster } from "./design/components/Toaster";
import { on } from "./lib/tauri";
import { setWindowShown } from "./design/render";
import { toast } from "./store/toastStore";

export default function App() {
  const [tab, setTab] = useState<Tab>("overview");
  const theme = useThemeStore((s) => s.theme);
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const showOnboarding = useSettingsStore(
    (s) => s.loaded && !!s.settings && !s.settings.has_completed_onboarding,
  );

  // Инициализация сторов и подписок — один раз при старте.
  useEffect(() => {
    const unlisten = initLogStream();
    useDpiStore.getState().bootstrap();
    useProxyStore.getState().bootstrap();
    useHostsStore.getState().bootstrap();
    useSettingsStore.getState().bootstrap();

    // Пауза анимаций при скрытии окна в трей (сигнал из Rust дополняет
    // Visibility API, который в WebView2 не всегда срабатывает на hide()).
    const unlistenVis = on.windowVisibility((visible) => setWindowShown(visible));

    // Значимые ошибки бэкенда (падение winws, сбой Глаз/прокси) — всплывают
    // тостом, чтобы пользователь заметил их, не открывая боковой лог.
    const unlistenErr = on.log((e) => {
      if (e.level === "error") toast.error(e.message, 6000);
    });

    // Прогреваем тяжёлые ленивые сцены (three/WebGL) на простое, чтобы первое
    // переключение темы не ждало загрузку чанка.
    const warm = () => {
      import("./design/components/RainScene3D");
      import("./design/components/RussiaHybrid");
    };
    const ric = (window as unknown as {
      requestIdleCallback?: (cb: () => void) => number;
    }).requestIdleCallback;
    const warmTimer = ric ? (ric(warm), 0) : window.setTimeout(warm, 1500);

    return () => {
      unlisten.then((fn) => fn());
      unlistenVis.then((fn) => fn());
      unlistenErr.then((fn) => fn());
      if (!ric) window.clearTimeout(warmTimer);
    };
  }, []);

  return (
    <MotionConfig reducedMotion={reduceMotion ? "always" : "user"}>
      <div
        data-theme={theme}
        data-reduce-motion={reduceMotion}
        className="relative h-screen w-screen overflow-hidden"
      >
        <ParallaxProvider>
          {/* Дальний план: фон движется против курсора. Оверскан по краям, чтобы
              сдвиг никогда не оголял углы. */}
          <Parallax depth={-12} className="absolute" style={{ inset: -32 }}>
            <HeroField />
          </Parallax>

          {/* Титлбар — чистый хром, без параллакса. */}
          <div className="absolute inset-x-0 top-0 z-20">
            <CustomTitleBar />
          </div>

          {/* Контент. */}
          <div className="absolute inset-0 top-10 flex">
          <NavRail active={tab} onSelect={setTab} />
          <main className="flex-1 overflow-hidden px-6 pb-6 pt-2">
            <AnimatePresence mode="wait">
              <motion.div
                key={tab}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -8 }}
                transition={{ duration: 0.22, ease: "easeOut" }}
                className="h-full"
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
