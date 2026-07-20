// Типизированный мост к Rust-бэкенду: обёртки invoke() и подписки на события.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
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
  /** Unix-время (сек) старта текущей сессии обхода; null = выключен. */
  started_at: number | null;
}

export interface ProxyStatus {
  running: boolean;
  link: string;
  lan_link: string | null;
  lan_published: boolean;
  lan_expiry_unix: number | null;
}

export interface RuntimeSnapshot {
  dpi: DpiStatus;
  proxy: ProxyStatus;
  brain: BrainStatus | null;
  adaptive: AdaptiveStatus | null;
}

export interface HostsStatus {
  provider: string;
  status: "installed" | "outdated" | "not_installed" | "offline";
  local_version: string;
  remote_version: string;
  rollback_available: boolean;
}

export interface VersionedSection<T> {
  revision: number;
  value: T;
}

export interface BootstrapSettings {
  settings: Settings;
  elevated: boolean;
  autostart: boolean;
}

export interface BootstrapSnapshot {
  schemaVersion: number;
  settings: VersionedSection<BootstrapSettings>;
  dpi: VersionedSection<DpiStatus>;
  proxy: VersionedSection<ProxyStatus>;
  brain: VersionedSection<BrainStatus | null>;
  adaptive: VersionedSection<AdaptiveStatus | null>;
  legacyReliability: VersionedSection<LegacyReliabilityStatus>;
  hosts: VersionedSection<HostsStatus>;
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
  zapret2_selected_categories: string[];
  selected_configs: Record<string, string>;
  proxy_port: number;
  fake_tls_domain: string;
  ai_provider: string;
  has_completed_onboarding: boolean;
  auto_recovery: boolean;
  reduce_motion: boolean;
  /** Глобальный хоткей вкл/выкл защиты (Tauri-акселератор, напр. "Ctrl+Shift+KeyO"). */
  hotkey_toggle: string;
  /** Таймаут публикации прокси в LAN (сек); 0 = без авто-закрытия. */
  lan_publish_secs: number;
  /** Выбранный DPI-движок: "legacy" (Zapret1) или "zapret2" (Beta). */
  dpi_engine: string;
  zapret2_level: number;
  /** Локальный bounded-поиск Safe Strategy DSL для Zapret2. */
  adaptive_strategy_enabled: boolean;
  adaptive_search_mode: "fast" | "balanced" | "deep";
}

export interface EngineOption {
  kind: string;
  version: string;
  beta: boolean;
  available: boolean;
  selected: boolean;
}

export type AdaptiveStrategyTrust = "prepared" | "recommended" | "confirmed";

export interface Zapret2ProfileDescriptor {
  category: string;
  profileId: string;
  source: "builtin" | "adaptive";
  transport: "http" | "tls" | "quic" | "tcp" | "udp";
  ports: string;
  hostlist: string | null;
  ipset: string | null;
  candidateId: string | null;
  verification: "verified" | "recommended" | "baseline" | "not_actively_verified";
  trust: AdaptiveStrategyTrust;
  evidenceSource: string | null;
  sourceCandidateId: string | null;
  sourceCategory: AdaptiveCategory | null;
  sourceTransport: AdaptiveTransport | null;
  recommendationReason: string | null;
  dataPlane: boolean;
  broadIpset: boolean;
}

// ─── Глаза / Мозг (контур надёжности) ───────────────────────────────────────

export type Verdict = "working" | "reset" | "blackhole";

/** Сырое per-flow наблюдение Глаз (событие `eyes://observation`). */
export interface Observation {
  flow_id: number;
  domain: string;
  dst_ip: string;
  local_port: number;
  remote_port: number;
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

export type LegacyReliabilityPhase =
  | "inactive"
  | "starting"
  | "observing"
  | "degraded"
  | "blind";

export type LegacyReliabilityLanePhase =
  | "observing"
  | "healthy"
  | "suspect"
  | "gate_pending"
  | "blocked_cooldown"
  | "sensor_unreliable";

export type LegacyReliabilityClassification =
  | "awaiting_evidence"
  | "working"
  | "dpi_suspected"
  | "dpi_blocked"
  | "offline"
  | "dns_failure"
  | "upstream_degraded"
  | "target_unavailable"
  | "service_slow"
  | "sensor_unreliable";

export type LegacyReliabilityConfidence = "none" | "low" | "medium" | "high";

export interface LegacyReliabilityEvidence {
  workingFlows: number;
  workingTargets: number;
  resetFlows: number;
  resetTargets: number;
  blackholeFlows: number;
  blackholeTargets: number;
}

export interface LegacyReliabilityLaneAssessment {
  category: string;
  activeConfig: string | null;
  laneGeneration: number;
  phase: LegacyReliabilityLanePhase;
  classification: LegacyReliabilityClassification;
  confidence: LegacyReliabilityConfidence;
  evidence: LegacyReliabilityEvidence;
  /** UX-only confirmation memory; never authorizes a configuration change. */
  workingConfirmedRecently: boolean;
  cooldownUntilMs: number | null;
}

export type LegacyReliabilityPresumedIntent =
  | {
      kind: "wait";
      reason: LegacyReliabilityClassification;
    }
  | {
      kind: "switch_lane";
      category: string;
      candidateConfig: string;
      reason: LegacyReliabilityClassification;
    }
  | {
      kind: "freeze_lane";
      category: string;
      untilMs: number;
      reason: LegacyReliabilityClassification;
    };

/**
 * Read-only lifecycle and per-category assessment projection of the active
 * Legacy Reliability Manager. `presumedIntent` is diagnostic in observe-only
 * mode and never means that a configuration change was executed.
 */
export interface LegacyReliabilityStatus {
  mode: "observe_only";
  phase: LegacyReliabilityPhase;
  activeCategories: string[];
  sessionId: number | null;
  sensorGeneration: number | null;
  lanes: LegacyReliabilityLaneAssessment[];
  presumedIntent: LegacyReliabilityPresumedIntent;
}

export type AdaptiveCategory = "discord" | "youtube_twitch" | "gaming";
export type AdaptiveTransport = "tls" | "quic";
export type AdaptivePhase =
  | "idle"
  | "suggested"
  | "discovering_quic"
  | "calibrating"
  | "searching"
  | "candidate_probe"
  | "temporary_verification"
  | "applying"
  | "rolling_back"
  | "applied"
  | "exhausted"
  | "probe_unreliable"
  | "quic_targets_unavailable"
  | "base_unhealthy"
  | "internal_error"
  | "cancelled";

export interface AdaptiveStatus {
  phase: AdaptivePhase;
  category: AdaptiveCategory | null;
  diagnosis:
    | "repeated_reset"
    | "tls_blackhole"
    | "quic_blackhole"
    | "probe_failure"
    | null;
  sessionId: number | null;
  attemptId: number | null;
  candidateId: string | null;
  candidateIndex: number | null;
  candidateTotal: number | null;
  verificationDeadlineMs: number | null;
  rollbackReason: string | null;
  transport: "tls" | "quic" | null;
  sessionMode: "comparison" | "recovery" | null;
  currentRound: number | null;
  totalRounds: number | null;
  failureStage:
    | "none"
    | "dns"
    | "tcp"
    | "tls"
    | "quic"
    | "https"
    | "eyes_reset"
    | "eyes_blackhole"
    | "spawn"
    | "stability"
    | null;
}

export interface AdaptiveSuggestion {
  category: AdaptiveCategory;
  reason: NonNullable<AdaptiveStatus["diagnosis"]>;
}

export interface AdaptiveRecommendationDescriptor {
  category: AdaptiveCategory;
  transport: AdaptiveTransport;
  candidateId: string;
  trust: "recommended";
  evidenceSource: string;
  sourceCandidateId: string;
  sourceCategory: AdaptiveCategory;
  sourceTransport: AdaptiveTransport;
  recommendationReason: string;
}

export interface AdaptiveTargetProbe {
  host: string;
  core: boolean;
  round: number;
  transport: "tls" | "quic";
  dnsOk: boolean;
  tcpOk: boolean;
  tlsOk: boolean;
  quicOk: boolean;
  httpsOk: boolean;
  httpStatus: number | null;
  latencyMs: number;
  failureStage: NonNullable<AdaptiveStatus["failureStage"]>;
  detail: string;
}

export interface AdaptiveProbeBatch {
  category: AdaptiveCategory;
  transport: "tls" | "quic";
  round: number;
  targets: AdaptiveTargetProbe[];
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
  /** Движок, подтвердивший конфиг ("legacy" | "zapret2"). Старые кэши → "legacy". */
  engine?: string;
}

// ─── Команды ──────────────────────────────────────────────────────────────

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  runtimeGetSnapshot: () => invoke<RuntimeSnapshot>("runtime_get_snapshot"),
  bootstrapGetSnapshot: () =>
    invoke<BootstrapSnapshot>("bootstrap_get_snapshot"),
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
  dpiEngineList: () => invoke<EngineOption[]>("dpi_engine_list"),
  dpiZapret2Profiles: (categories: string[]) =>
    invoke<Zapret2ProfileDescriptor[]>("dpi_zapret2_profiles", { categories }),
  dpiEngineSet: (engine: string) => invoke<void>("dpi_engine_set", { engine }),
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
  proxyCloseLan: () => invoke<void>("proxy_close_lan"),
  proxyLink: () => invoke<string>("proxy_link"),
  openExternalUrl: (url: string) => invoke<void>("open_external_url", { url }),

  hostsStatus: (provider: string) =>
    invoke<HostsStatus>("hosts_status", { provider }),
  hostsInstall: (provider: string) =>
    invoke<void>("hosts_install", { provider }),
  hostsUninstall: () => invoke<void>("hosts_uninstall"),
  hostsRestore: (provider: string) =>
    invoke<void>("hosts_restore", { provider }),

  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Settings>) =>
    invoke<Settings>("update_settings", { patch }),
  // Меняет глобальный хоткей (пустая строка = выключить). Формат — Tauri-
  // акселератор с Code-именем клавиши ("Ctrl+Shift+KeyO").
  setHotkey: (hotkey: string) => invoke<void>("set_hotkey", { hotkey }),

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
  adaptiveGetStatus: () =>
    invoke<AdaptiveStatus | null>("adaptive_get_status"),
  adaptiveStartSearch: (category: AdaptiveCategory, transport: AdaptiveTransport) =>
    invoke<void>("adaptive_start_search", { category, transport }),
  adaptiveGetRecommendation: (
    category: AdaptiveCategory,
    transport: AdaptiveTransport,
  ) =>
    invoke<AdaptiveRecommendationDescriptor | null>("adaptive_get_recommendation", {
      category,
      transport,
    }),
  adaptiveApplyRecommendation: (
    category: AdaptiveCategory,
    transport: AdaptiveTransport,
  ) =>
    invoke<AdaptiveRecommendationDescriptor>("adaptive_apply_recommendation", {
      category,
      transport,
    }),
  adaptiveCancelSearch: () => invoke<void>("adaptive_cancel_search"),
  adaptiveConfirmCandidate: (sessionId: number, candidateId: string) =>
    invoke<void>("adaptive_confirm_candidate", { sessionId, candidateId }),
  adaptiveRejectCandidate: (sessionId: number, candidateId: string) =>
    invoke<void>("adaptive_reject_candidate", { sessionId, candidateId }),
  adaptiveResetSaved: (category: AdaptiveCategory) =>
    invoke<void>("adaptive_reset_saved", { category }),
};

// ─── События ──────────────────────────────────────────────────────────────


export const on = {
  log: (cb: (e: LogEvent) => void): Promise<UnlistenFn> =>
    listen<LogEvent>("log", (e) => cb(e.payload)),
  dpiStatus: (cb: (e: DpiStatus) => void): Promise<UnlistenFn> =>
    listen<VersionedSection<DpiStatus>>("dpi-status", (e) => {
      cb(e.payload.value);
    }),
  dpiStatusVersioned: (
    cb: (section: VersionedSection<DpiStatus>) => void,
  ): Promise<UnlistenFn> =>
    listen<VersionedSection<DpiStatus>>("dpi-status", (e) => cb(e.payload)),
  proxyStatus: (cb: (e: ProxyStatus) => void): Promise<UnlistenFn> =>
    listen<VersionedSection<ProxyStatus>>("proxy-status", (e) => {
      cb(e.payload.value);
    }),
  proxyStatusVersioned: (
    cb: (section: VersionedSection<ProxyStatus>) => void,
  ): Promise<UnlistenFn> =>
    listen<VersionedSection<ProxyStatus>>("proxy-status", (e) => cb(e.payload)),
  brainStatus: (cb: (s: BrainStatus) => void): Promise<UnlistenFn> =>
    listen<VersionedSection<BrainStatus>>("brain://status", (e) => {
      cb(e.payload.value);
    }),
  brainStatusVersioned: (
    cb: (section: VersionedSection<BrainStatus>) => void,
  ): Promise<UnlistenFn> =>
    listen<VersionedSection<BrainStatus>>("brain://status", (e) => cb(e.payload)),
  legacyReliabilityStatus: (
    cb: (status: LegacyReliabilityStatus) => void,
  ): Promise<UnlistenFn> =>
    listen<VersionedSection<LegacyReliabilityStatus>>(
      "legacy-reliability://status",
      (e) => cb(e.payload.value),
    ),
  legacyReliabilityStatusVersioned: (
    cb: (section: VersionedSection<LegacyReliabilityStatus>) => void,
  ): Promise<UnlistenFn> =>
    listen<VersionedSection<LegacyReliabilityStatus>>(
      "legacy-reliability://status",
      (e) => cb(e.payload),
    ),
  adaptiveStatus: (cb: (s: AdaptiveStatus) => void): Promise<UnlistenFn> =>
    listen<VersionedSection<AdaptiveStatus>>("adaptive://status", (e) => {
      cb(e.payload.value);
    }),
  adaptiveStatusVersioned: (
    cb: (section: VersionedSection<AdaptiveStatus>) => void,
  ): Promise<UnlistenFn> =>
    listen<VersionedSection<AdaptiveStatus>>("adaptive://status", (e) =>
      cb(e.payload),
    ),
  adaptiveSuggestion: (
    cb: (suggestion: AdaptiveSuggestion) => void,
  ): Promise<UnlistenFn> =>
    listen<AdaptiveSuggestion>("adaptive://suggestion", (e) => cb(e.payload)),
  adaptiveProbe: (cb: (probe: AdaptiveProbeBatch) => void): Promise<UnlistenFn> =>
    listen<AdaptiveProbeBatch>("adaptive://probe", (e) => cb(e.payload)),
  // Rust сообщает о скрытии/показе окна в трей — дополняет Visibility API для
  // паузы анимаций (WebView2 не всегда шлёт visibilitychange на hide()).
  windowVisibility: (cb: (visible: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>("window-visibility", (e) => cb(e.payload)),
};

// ─── Runtime snapshots ────────────────────────────────────────────────────
// Legacy snapshot stays available for compatibility; launcher hydration and
// resume use independently versioned bootstrap sections.
export const runtime = {
  snapshot: (): Promise<RuntimeSnapshot> => api.runtimeGetSnapshot(),
  bootstrap: (): Promise<BootstrapSnapshot> => api.bootstrapGetSnapshot(),
};

// ─── Утилиты окна / системы ────────────────────────────────────────────────

export const win = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  // Показ/скрытие именно веб-вью: в свёрнутом (iconic) окне композитор WebView2
  // продолжает рисовать кадры — hide() гасит рендер до ~0% CPU, show() возвращает.
  showWebview: () => getCurrentWebview().show(),
  hideWebview: () => getCurrentWebview().hide(),
  onFocusChanged: (cb: (focused: boolean) => void): Promise<UnlistenFn> =>
    getCurrentWindow().onFocusChanged(({ payload }) => cb(payload)),
};

export const clipboard = { write: writeText };
export const opener = { open: openUrl };
