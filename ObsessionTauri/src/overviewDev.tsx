// Development-only presentation of the production component with sample data.
// Not a Vite build entry; never imports App or starts the runtime/bootstrap.
import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { MotionConfig } from "framer-motion";
import { ScreenDeck, useScreenNavigation } from "./design/components/ScreenDeck";
import { HeroField } from "./design/components/HeroField";
import { NavRail } from "./design/components/NavRail";
import { THEMES, useThemeStore, type Theme } from "./store/themeStore";
import { useDpiStore } from "./store/dpiStore";
import { useProxyStore } from "./store/proxyStore";
import { useHostsStore } from "./store/hostsStore";
import { useSettingsStore } from "./store/settingsStore";
import { api, type AppConfig, type DpiTestReport, type EngineOption } from "./lib/tauri";
import { invokeBrowserPreview } from "./lib/browserPreview";
import { useMotionOff } from "./design/render";
import { useObsessionVisualPhase } from "./design/useObsessionVisualPhase";
import { ThemeAtmosphere, ThemeAtmosphereBackdrop, AtmospherePreviewButton } from "./design/components/ThemeAtmosphere";
import { StartupIntro, StartupIntroReplayButton } from "./design/components/StartupIntro";
import { useStartupIntro } from "./design/startupIntro";
import "./styles/fonts.css";
import "./styles/globals.css";

async function prepareOverviewPreview() {
  if (!import.meta.env.DEV || "__TAURI_INTERNALS__" in window) return;
  api.getNetworkIdentity = async () => ({ online: true, org: "AS12389 Ростелеком", asn_region: "Россия", gateway_mac_masked: "••:••:••:12:34:56" });
  const [savedSettings, emptyConfig, previewEngines] = await Promise.all([
    invokeBrowserPreview<Awaited<ReturnType<typeof api.getSettings>>>("get_settings"),
    invokeBrowserPreview<AppConfig>("get_config"),
    invokeBrowserPreview<EngineOption[]>("dpi_engine_list"),
  ]);
  const settings = { ...savedSettings, selected_categories: ["discord", "youtube_twitch"] };
  const sampleConfigs = {
    discord: ["discord_1.conf", "discord_2.conf", "discord_3.conf"],
    youtube_twitch: ["youtube_twitch_1.conf", "youtube_twitch_2.conf"],
    gaming: ["gaming_1.conf", "gaming_2.conf"],
    universal: ["universal_1.conf", "universal_2.conf"],
  };
  const config: AppConfig = {
    ...emptyConfig,
    categories: Object.keys(sampleConfigs),
    configs: Object.fromEntries(Object.entries(sampleConfigs).map(([category, files]) => [
      category, emptyConfig.configs[category]?.length ? emptyConfig.configs[category] : files,
    ])),
  };
  useSettingsStore.setState((state) => ({
    settings,
    loaded: true,
    protectedRuntimeAvailable: true,
    protectedDpiAvailable: true,
    protectedRuntime: { ...state.protectedRuntime, serviceAvailable: true, dpi: true },
  }));
  useDpiStore.getState().initialize(config, settings);
  const previewTheme = new URLSearchParams(window.location.search).get("theme");
  useThemeStore.setState({ theme: THEMES.some((entry) => entry.id === previewTheme) ? previewTheme as Theme : "goldenmeadow" });
  const toggle = async (active: boolean) => {
    if (useDpiStore.getState().transitioning || useDpiStore.getState().testing) return;
    useDpiStore.setState({ transitioning: true });
    await new Promise((resolve) => setTimeout(resolve, 700));
    useDpiStore.setState((state) => ({
      active,
      transitioning: false,
      startedAt: active ? Date.now() : null,
      processes: active ? state.selectedCategories.map((category, index) => ({
        pid: 1000 + index, category, config_file: state.selectedConfigs[category] ?? "",
      })) : [],
    }));
  };
  // Sample reports belong only to this lab; no runtime or network test is started.
  const test = async (autoConfigure = false) => {
    const state = useDpiStore.getState();
    if (state.active || state.transitioning || state.testing) return;
    useDpiStore.setState({ testing: true, testCancel: false, error: "", testResults: {}, testReports: {} });
    try {
      for (const category of state.selectedCategories) {
        const file = autoConfigure ? config.configs[category]?.[0] : state.selectedConfigs[category];
        if (!file || useDpiStore.getState().testCancel) break;
        useDpiStore.setState({ testingLabel: `${category}: ${file}` });
        await new Promise((resolve) => setTimeout(resolve, 550));
        if (useDpiStore.getState().testCancel) break;
        const report: DpiTestReport = {
          passed: true,
          status: "passed",
          checks: [{
            url: category === "discord" ? "https://discord.com/api/v10/gateway" : "https://www.youtube.com/",
            passed: true, elapsed_ms: 180, bytes: 4096, attempts: 1, http_status: 200, error: null,
          }],
        };
        useDpiStore.setState((current) => ({
          testReports: { ...current.testReports, [file]: report },
          testResults: { ...current.testResults, [file]: true },
          ...(autoConfigure ? { selectedConfigs: { ...current.selectedConfigs, [category]: file } } : {}),
        }));
      }
    } finally {
      useDpiStore.setState({ testing: false, testingLabel: "", testCancel: false });
    }
  };
  useDpiStore.setState({
    active: false,
    engines: previewEngines.length ? previewEngines : [{ kind: "legacy", version: "1.0", beta: false, available: true, selected: true }],
    start: () => toggle(true),
    stop: () => toggle(false),
    testAll: () => test(),
    autoConfigure: () => test(true),
  });
  useProxyStore.setState({ available: true });
  useHostsStore.setState({ status: "installed", provider: "malw", localVersion: "29 августа 2026" });
  if (new URLSearchParams(window.location.search).get("intro") === "full") useStartupIntro.getState().replay();
  createRoot(document.getElementById("root")!).render(<React.StrictMode><OverviewPreview /></React.StrictMode>);
}

function OverviewPreview() {
  const theme = useThemeStore((state) => state.theme);
  const [quiet, setQuiet] = useState(false);
  const { tab, selectTab, deckRef } = useScreenNavigation();
  const [edgeCase, setEdgeCase] = useState(false);
  const motionOff = useMotionOff();
  const phase = useObsessionVisualPhase();
  const startup = useStartupIntro((s) => s.request);
  const arriving = useStartupIntro((s) => s.arriving);
  return (
    <MotionConfig reducedMotion={motionOff ? "always" : "never"}>
      <div data-theme={theme} data-startup-shell data-startup-stage={startup ? startup.mode === "quick" ? "quick" : arriving ? "arriving" : "full" : undefined} className="relative h-screen overflow-hidden text-ink">
        <ThemeAtmosphere theme={theme} phase={phase} screen={tab}>
        <div className="absolute inset-0"><HeroField theme={theme} frozen={quiet} /></div>
        <ThemeAtmosphereBackdrop />
        <div className="relative z-10 flex h-10 items-center justify-between gap-3 px-4 text-xs" style={{ background: "#24232beb", color: "#f2eadc" }}>
          <label className="flex items-center gap-2">Тема
            <select aria-label="Тема макета" value={theme} onChange={(event) => useThemeStore.setState({ theme: event.target.value as Theme })}
              style={{ background: "#37343f", color: "#f2eadc", borderRadius: 6, padding: "3px 8px" }}>
              {THEMES.map((entry) => <option key={entry.id} value={entry.id}>{entry.label}{entry.secret ? " · секретная" : ""}</option>)}
            </select>
            <span className="opacity-60">Макет</span>
          </label>
          <div className="flex gap-4">
            <StartupIntroReplayButton />
            <AtmospherePreviewButton />
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
        <div data-startup-content className="absolute inset-0 top-10 z-10 flex">
          <NavRail active={tab} onSelect={selectTab} />
          <main className="h-full min-h-0 min-w-0 flex-1 overflow-hidden px-6 pb-6 pt-2"><ScreenDeck ref={deckRef} tab={tab} onSelect={selectTab} /></main>
        </div>
        <StartupIntro theme={theme} />
        </ThemeAtmosphere>
      </div>
    </MotionConfig>
  );
}

void prepareOverviewPreview();
