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

export interface Settings {
  minimize_to_tray: boolean;
  start_minimized: boolean;
  selected_categories: string[];
  selected_configs: Record<string, string>;
  proxy_port: number;
  fake_tls_domain: string;
  ai_provider: string;
  has_completed_onboarding: boolean;
  locale: string;
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
  dpiDetectOrphaned: () => invoke<number[]>("dpi_detect_orphaned"),
  dpiEmergencyKill: () => invoke<void>("dpi_emergency_kill"),

  proxyAvailable: () => invoke<boolean>("proxy_available"),
  proxyStart: (port: number, fakeTlsDomain: string) =>
    invoke<string>("proxy_start", { port, fakeTlsDomain }),
  proxyStop: () => invoke<void>("proxy_stop"),
  proxyLink: () => invoke<string>("proxy_link"),

  hostsStatus: (provider: string) =>
    invoke<HostsStatus>("hosts_status", { provider }),
  hostsInstall: (provider: string) =>
    invoke<void>("hosts_install", { provider }),
  hostsUninstall: () => invoke<void>("hosts_uninstall"),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<void>("save_settings", { settings }),

  getProfiles: () => invoke<Profile[]>("get_profiles"),
  saveProfile: (profile: Profile) =>
    invoke<Profile[]>("save_profile", { profile }),
  deleteProfile: (id: string) =>
    invoke<Profile[]>("delete_profile", { id }),
};

// ─── События ──────────────────────────────────────────────────────────────

export const on = {
  log: (cb: (e: LogEvent) => void): Promise<UnlistenFn> =>
    listen<LogEvent>("log", (e) => cb(e.payload)),
  dpiStatus: (cb: (e: DpiStatus) => void): Promise<UnlistenFn> =>
    listen<DpiStatus>("dpi-status", (e) => cb(e.payload)),
  proxyStatus: (cb: (e: ProxyStatus) => void): Promise<UnlistenFn> =>
    listen<ProxyStatus>("proxy-status", (e) => cb(e.payload)),
};

// ─── Утилиты окна / системы ────────────────────────────────────────────────

export const win = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
};

export const clipboard = { write: writeText };
export const opener = { open: openUrl };
