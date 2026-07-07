import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { HeroField } from "./design/components/HeroField";
import { SnowOverlay } from "./design/components/SnowOverlay";
import { SakuraOverlay } from "./design/components/SakuraOverlay";
import { ParallaxProvider, Parallax } from "./design/parallax";
import { CustomTitleBar } from "./design/components/CustomTitleBar";
import { NavRail, type Tab } from "./design/components/NavRail";
import { DpiScreen } from "./screens/Dpi";
import { AiScreen } from "./screens/Ai";
import { TelegramScreen } from "./screens/Telegram";
import { SoonScreen } from "./screens/Soon";
import { SettingsScreen } from "./screens/Settings";
import { ProfilesScreen } from "./screens/Profiles";

import { initLogStream } from "./store/logStore";
import { useDpiStore } from "./store/dpiStore";
import { useProxyStore } from "./store/proxyStore";
import { useHostsStore } from "./store/hostsStore";
import { useThemeStore } from "./store/themeStore";
import { useSecretStore } from "./store/secretStore";

export default function App() {
  const [tab, setTab] = useState<Tab>("dpi");
  const theme = useThemeStore((s) => s.theme);
  const overlays = useSecretStore((s) => s.overlays);

  // Инициализация сторов и подписок — один раз при старте.
  useEffect(() => {
    const unlisten = initLogStream();
    useDpiStore.getState().bootstrap();
    useProxyStore.getState().bootstrap();
    useHostsStore.getState().bootstrap();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div data-theme={theme} className="relative h-screen w-screen overflow-hidden">
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
              {tab === "dpi" && <DpiScreen />}
              {tab === "ai" && <AiScreen />}
              {tab === "telegram" && <TelegramScreen />}
              {tab === "lists" && <SoonScreen title="Списки" />}
              {tab === "profiles" && <ProfilesScreen />}
              {tab === "settings" && <SettingsScreen />}
            </motion.div>
          </AnimatePresence>
        </main>
        </div>

        {/* Пасхальные оверлеи поверх всего (pointer-events-none). */}
        {overlays.snow && <SnowOverlay />}
        {overlays.sakura && <SakuraOverlay />}
      </ParallaxProvider>
    </div>
  );
}
