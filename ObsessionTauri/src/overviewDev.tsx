// Development-only presentation of the production component with sample data.
// Not a Vite build entry; never imports App or starts the runtime/bootstrap.
import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { MotionConfig } from "framer-motion";
import { OverviewScreen } from "./screens/Overview";
import { HeroField } from "./design/components/HeroField";
import { NavRail, type Tab } from "./design/components/NavRail";
import { THEMES, useThemeStore, type Theme } from "./store/themeStore";
import { useDpiStore } from "./store/dpiStore";
import { useProxyStore } from "./store/proxyStore";
import { useHostsStore } from "./store/hostsStore";
import { useSettingsStore } from "./store/settingsStore";
import { api } from "./lib/tauri";
import { invokeBrowserPreview } from "./lib/browserPreview";
import { useMotionOff } from "./design/render";
import "./styles/fonts.css";
import "./styles/globals.css";

async function prepareOverviewPreview() {
  if (!import.meta.env.DEV || "__TAURI_INTERNALS__" in window) return;
  api.getNetworkIdentity = async () => ({ online: true, org: "AS12389 Ростелеком", asn_region: "Россия", gateway_mac_masked: "••:••:••:12:34:56" });
  const settings = await invokeBrowserPreview<Awaited<ReturnType<typeof api.getSettings>>>("get_settings");
  useSettingsStore.setState({ settings, loaded: true });
  useThemeStore.setState({ theme: "goldenmeadow" });
  const toggle = async (active: boolean) => {
    if (useDpiStore.getState().transitioning) return;
    useDpiStore.setState({ transitioning: true });
    await new Promise((resolve) => setTimeout(resolve, 700));
    useDpiStore.setState({ active, transitioning: false, startedAt: active ? Date.now() : null });
  };
  useDpiStore.setState({ active: false, selectedCategories: ["discord", "youtube_twitch"], start: () => toggle(true), stop: () => toggle(false) });
  useProxyStore.setState({ available: true });
  useHostsStore.setState({ status: "installed", provider: "malw", localVersion: "29 августа 2026" });
  createRoot(document.getElementById("root")!).render(<React.StrictMode><OverviewPreview /></React.StrictMode>);
}

function OverviewPreview() {
  const theme = useThemeStore((state) => state.theme);
  const [quiet, setQuiet] = useState(false);
  const [tab, setTab] = useState<Tab>("overview");
  const [edgeCase, setEdgeCase] = useState(false);
  const motionOff = useMotionOff();
  return (
    <MotionConfig reducedMotion={motionOff ? "always" : "never"}>
      <div data-theme={theme} className="relative h-screen overflow-hidden text-ink">
        <div className="absolute inset-0"><HeroField theme={theme} frozen={quiet} /></div>
        <div className="relative z-10 flex h-10 items-center justify-between gap-3 px-4 text-xs" style={{ background: "#24232beb", color: "#f2eadc" }}>
          <label className="flex items-center gap-2">Тема
            <select aria-label="Тема макета" value={theme} onChange={(event) => useThemeStore.setState({ theme: event.target.value as Theme })}
              style={{ background: "#37343f", color: "#f2eadc", borderRadius: 6, padding: "3px 8px" }}>
              {THEMES.map((entry) => <option key={entry.id} value={entry.id}>{entry.label}{entry.secret ? " · секретная" : ""}</option>)}
            </select>
            <span className="opacity-60">Макет</span>
          </label>
          <div className="flex gap-4">
            <label className="flex items-center gap-1.5"><input type="checkbox" checked={edgeCase} onChange={(event) => {
              setEdgeCase(event.target.checked);
              useHostsStore.setState({ status: event.target.checked ? "outdated" : "installed" });
              useProxyStore.setState({ running: event.target.checked });
            }} />Другие статусы</label>
            <label className="flex items-center gap-1.5"><input type="checkbox" checked={quiet} onChange={(event) => {
              setQuiet(event.target.checked);
              useSettingsStore.setState((state) => ({ settings: state.settings ? { ...state.settings, reduce_motion: event.target.checked } : null }));
            }} />Меньше анимаций</label>
          </div>
        </div>
        <div className="absolute inset-0 top-10 flex">
          <NavRail active={tab} onSelect={setTab} />
          <main className="min-w-0 flex-1 overflow-hidden px-6 pb-6 pt-2"><OverviewScreen /></main>
        </div>
      </div>
    </MotionConfig>
  );
}

void prepareOverviewPreview();
