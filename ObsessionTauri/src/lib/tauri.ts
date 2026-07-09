// Типизированный мост к Rust-бэкенду: обёртки invoke() и подписки на события.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";

// ─── Типы (зеркалят Rust-payload'ы) ──────────────────────────────────────

export interface AppConfig {
  categories: string[];
  configs: Record<string, string[]>;
  lists: string[];
}

export interface DpiProc {
  pid: number;
  category: string;
  config_file: string;
}

export interface DpiStatus {
  active: boolean;
  processes: DpiProc[];
}

export interface ProxyStatus {
  running: boolean;
  link: string;
  lan_link: string | null;
}

export interface HostsStatus {
  provider: string;
  status: "installed" | "outdated" | "not_installed" | "offline";
  local_version: string;
  remote_version: string;
}

export interface LogEvent {
  level: "info" | "success" | "warn" | "error";
  source: string;
  message: string;
  ts: string;
}

export interface DiagResult {
  name: string;
  url: string;
  ok: boolean;
  ms: number;
}

export interface Profile {
  id: string;
  name: string;
  selected_categories: string[];
  selected_configs: Record<string, string>;
  proxy_port: number;
  fake_tls_domain: string;
  ai_provider: string;
}

export interface ListInfo {
  name: string;
  entries: number;
  bytes: number;
  kind: "domains" | "ipset";
}

export interface Settings {
  minimize_to_tray: boolean;
  start_minimized: boolean;
  selected_categories: string[];
  selected_configs: Record<string, string>;
  proxy_port: number;
  fake_tls_domain: string;
  ai_provider: string;
  has_completed_onboarding: boolean;
  auto_recovery: boolean;
  reduce_motion: boolean;
}

// ─── Глаза / Мозг (контур надёжности) ───────────────────────────────────────

export type Verdict = "working" | "reset" | "blackhole";

/** Сырое per-flow наблюдение Глаз (событие `eyes://observation`). */
export interface Observation {
  domain: string;
  dst_ip: string;
  local_port: number;
  verdict: Verdict;
  evidence: string;
  ts_ms: number;
}

/** Агрегированный статус Мозга (событие `brain://status`, camelCase из serde). */
export interface BrainStatus {
  enabled: boolean;
  phase:
    | "idle"
    | "confirming"
    | "healthy"
    | "suspect"
    | "switching"
    | "frozen"
    | "exhausted";
  category: string | null;
  currentConf: string | null;
  ladderLevel: "l1" | "l2" | "l3" | "none";
  frozenUntilMs: number | null;
  backoffSecs: number | null;
  asnRegion: string | null;
  gatewayMacMasked: string | null;
}

/** Идентичность текущей сети (для дашборда «Обзор»). */
export interface NetworkInfo {
  online: boolean;
  asn_region: string | null;
  org: string | null;
  gateway_mac_masked: string | null;
}

/** Запись надёжности конфига из L1-кэша (netcache). */
export interface ConfStat {
  conf: string;
  success_count: number;
  confirmed_at: number;
}

// ─── Команды ──────────────────────────────────────────────────────────────

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  isElevated: () => invoke<boolean>("is_elevated"),
  diagnose: () => invoke<DiagResult[]>("diagnose"),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enable: boolean) => invoke<void>("set_autostart", { enable }),

  dpiStart: (configs: { category: string; config_file: string }[]) =>
    invoke<number[]>("dpi_start", { configs }),
  dpiStop: () => invoke<void>("dpi_stop"),
  dpiTest: (category: string, configFile: string) =>
    invoke<boolean>("dpi_test", { category, configFile }),
  dpiTestCancel: () => invoke<void>("dpi_test_cancel"),
  dpiDetectOrphaned: () => invoke<number[]>("dpi_detect_orphaned"),
  dpiEmergencyKill: () => invoke<void>("dpi_emergency_kill"),
  getNetworkIdentity: () => invoke<NetworkInfo>("get_network_identity"),
  getNetcacheStats: () => invoke<Record<string, ConfStat>>("get_netcache_stats"),
  recordWorkingConfig: (category: string, conf: string) =>
    invoke<void>("record_working_config", { category, conf }),

  proxyAvailable: () => invoke<boolean>("proxy_available"),
  proxyStart: (port: number, fakeTlsDomain: string) =>
    invoke<string>("proxy_start", { port, fakeTlsDomain }),
  proxyStop: () => invoke<void>("proxy_stop"),
  proxyLink: () => invoke<string>("proxy_link"),
  openExternalUrl: (url: string) => invoke<void>("open_external_url", { url }),

  hostsStatus: (provider: string) =>
    invoke<HostsStatus>("hosts_status", { provider }),
  hostsInstall: (provider: string) =>
    invoke<void>("hosts_install", { provider }),
  hostsUninstall: () => invoke<void>("hosts_uninstall"),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<void>("save_settings", { settings }),

  listsAll: () => invoke<ListInfo[]>("lists_all"),
  readList: (name: string) => invoke<string>("read_list", { name }),
  saveList: (name: string, content: string) =>
    invoke<void>("save_list", { name, content }),
  createList: (name: string) => invoke<ListInfo[]>("create_list", { name }),
  deleteList: (name: string) => invoke<ListInfo[]>("delete_list", { name }),

  getProfiles: () => invoke<Profile[]>("get_profiles"),
  saveProfile: (profile: Profile) =>
    invoke<Profile[]>("save_profile", { profile }),
  deleteProfile: (id: string) =>
    invoke<Profile[]>("delete_profile", { id }),

  brainSetEnabled: (enabled: boolean) =>
    invoke<void>("brain_set_enabled", { enabled }),
  brainGetStatus: () => invoke<BrainStatus | null>("brain_get_status"),
};

// ─── События ──────────────────────────────────────────────────────────────

export const on = {
  log: (cb: (e: LogEvent) => void): Promise<UnlistenFn> =>
    listen<LogEvent>("log", (e) => cb(e.payload)),
  dpiStatus: (cb: (e: DpiStatus) => void): Promise<UnlistenFn> =>
    listen<DpiStatus>("dpi-status", (e) => cb(e.payload)),
  proxyStatus: (cb: (e: ProxyStatus) => void): Promise<UnlistenFn> =>
    listen<ProxyStatus>("proxy-status", (e) => cb(e.payload)),
  eyesObservation: (cb: (o: Observation) => void): Promise<UnlistenFn> =>
    listen<Observation>("eyes://observation", (e) => cb(e.payload)),
  brainStatus: (cb: (s: BrainStatus) => void): Promise<UnlistenFn> =>
    listen<BrainStatus>("brain://status", (e) => cb(e.payload)),
  // Rust сообщает о скрытии/показе окна в трей — дополняет Visibility API для
  // паузы анимаций (WebView2 не всегда шлёт visibilitychange на hide()).
  windowVisibility: (cb: (visible: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>("window-visibility", (e) => cb(e.payload)),
};

// ─── Утилиты окна / системы ────────────────────────────────────────────────

export const win = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
};

export const clipboard = { write: writeText };
export const opener = { open: openUrl };
