// Типизированный мост к Rust-бэкенду: обёртки invoke() и подписки на события.

import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import {
  listen,
  type EventCallback,
  type EventName,
  type UnlistenFn,
} from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invokeBrowserPreview } from "./browserPreview";
import { trackBackendRequest } from "./backendActivity";

// ─── Типы (зеркалят Rust-payload'ы) ──────────────────────────────────────

export interface AppConfig {
  categories: string[];
  configs: Record<string, string[]>;
  lists: string[];
}

export interface UpdateCheck {
  current_version: string;
  latest_version: string;
  available: boolean;
  notes: string;
  release_url: string;
}

export interface UpdateProgress {
  downloaded: number;
  total: number;
}

export interface DpiTestReport {
  passed: boolean;
  status: "passed" | "partial" | "failed" | "cancelled";
  checks: Array<{
    url: string;
    passed: boolean;
    elapsed_ms: number;
    bytes: number;
    attempts: number;
    http_status: number | null;
    error: string | null;
  }>;
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
}

export interface HostsStatus {
  provider: string;
  status: "installed" | "outdated" | "not_installed" | "offline" | "external";
  local_version: string;
  remote_version: string;
  rollback_available: boolean;
}

export type AiService = "chatgpt" | "claude" | "gemini";
export type AiRouteHealth =
  | "working"
  | "unavailable"
  | "inconclusive"
  | "unchecked";
export type AiRouteKind = "preferred" | "fallback" | "direct";
export type HostsRouteProvider = "malw" | "geohide" | "astracat" | "comss" | "xbox";
export type GeminiRoutePreference = "auto" | "geohide" | "astracat";
export type AiRouteFailureReason =
  | "timeout"
  | "tls"
  | "dns"
  | "routeMissing"
  | "offline"
  | "externalChange";

export interface AiServiceRouteHealth {
  service: AiService;
  health: AiRouteHealth;
  route: AiRouteKind;
  provider: HostsRouteProvider | null;
  reason: AiRouteFailureReason | null;
}

export interface HostsHealthSnapshot {
  preferredProvider: HostsRouteProvider;
  installed: boolean;
  checkedAtUnix: number | null;
  repairRecommended: boolean;
  services: AiServiceRouteHealth[];
}

export interface VersionedSection<T> {
  revision: number;
  value: T;
}

export interface ProtectedRuntimeCapabilities {
  serviceAvailable: boolean;
  serviceVersion: string | null;
  dpi: boolean;
  zapret2: boolean;
  eyesEvents: boolean;
  legacyReliabilityControls: boolean;
  legacyReliability: boolean;
  hosts: boolean;
  proxyLanFirewall: boolean;
  systemDnsRecovery: boolean;
}

export interface SystemDnsStatus {
  active: boolean;
  restoreAvailable: boolean;
  provider: "cloudflare" | "google" | null;
  interfaceName: string | null;
}

export interface BootstrapSettings {
  settings: Settings;
  elevated: boolean;
  autostart: boolean;
  protectedRuntime: ProtectedRuntimeCapabilities;
  protectedRuntimeAvailable: boolean;
  protectedDpiAvailable: boolean;
  protectedLegacyReliabilityAvailable: boolean;
}

export interface BootstrapSnapshot {
  schemaVersion: number;
  settings: VersionedSection<BootstrapSettings>;
  dpi: VersionedSection<DpiStatus>;
  proxy: VersionedSection<ProxyStatus>;
  brain: VersionedSection<BrainStatus | null>;
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
  /** One-way backend migration marker; not an Automatic opt-in control. */
  legacy_reliability_migration_version: number;
  /** Master switch for Legacy Eyes, Manager and recovery coordination. */
  legacy_reliability_enabled: boolean;
  /** Legacy reliability rollout. Automatic is an explicit Legacy-only opt-in. */
  legacy_reliability_mode: "observe_only" | "assisted" | "automatic";
  /** Global safety latch for new automatic Legacy attempts. */
  legacy_automatic_paused: boolean;
  /** Categories excluded from future automatic Legacy attempts by the user. */
  legacy_reliability_frozen_categories: string[];
  reduce_motion: boolean;
  /** Глобальный хоткей вкл/выкл защиты (Tauri-акселератор, напр. "Ctrl+Shift+KeyO"). */
  hotkey_toggle: string;
  /** Таймаут публикации прокси в LAN (сек); 0 = без авто-закрытия. */
  lan_publish_secs: number;
  /** Выбранный DPI-движок: "legacy" (Zapret1) или "zapret2" (Beta). */
  dpi_engine: string;
  zapret2_level: number;
  zapret2_clienthello: boolean;
  zapret2_profiles: Zapret2ProfileChoice[];
  zapret2_pick_mode: string;
}

export interface EngineOption {
  kind: string;
  version: string;
  beta: boolean;
  available: boolean;
  selected: boolean;
}

export interface Zapret2ProfileDescriptor {
  category: string;
  profileId: string;
  source: "builtin";
  transport: "http" | "tls" | "quic" | "tcp" | "udp";
  ports: string;
  hostlist: string | null;
  ipset: string | null;
  dataPlane: boolean;
  broadIpset: boolean;
}

export interface Zapret2ProfileChoice {
  category: string;
  transport: string;
  profileId: string;
}

export interface Zapret2CatalogProfile extends Zapret2ProfileChoice {
  compositionEligible?: boolean;
  composition?: { apiProfileId: string; cdnProfileId: string } | null;
  name: string;
  experimental: boolean;
  dataPlane: boolean;
  level: number;
  selected: boolean;
  candidate?: {
    label: string;
    family: string;
    urls: string[];
    evidence: "author_report" | "blockcheck_report" | "derived";
    notes: string;
  } | null;
}

export type Zapret2PickMode = "fast" | "balanced" | "deep";

export interface Zapret2PickerResult {
  category: string;
  transport: string;
  profileId?: string;
  status: "passed" | "exhausted" | "cancelled" | "tie" | "insufficient" | "partial" | "inconclusive" | "aborted";
  checked: number;
  total: number;
  report?: DpiTestReport;
  failure?: string | null;
  historyWarning?: string | null;
  tournament?: Zapret2Tournament;
}

export interface Zapret2FunctionSummary {
  function: string;
  label: string;
  required: boolean;
  passed: number;
  failed: number;
  inconclusive: number;
  notTested: number;
  cancelled: number;
  proof: string;
}

export interface Zapret2CandidateSummary {
  composition?: { apiProfileId: string; cdnProfileId: string } | null;
  profileId: string;
  label: string;
  family: string;
  fingerprint: string;
  rounds: number;
  completeRounds: number;
  routingVerifiedRounds?: number;
  requiredConfirmations: number;
  status: "confirmed" | "partial" | "inconclusive" | "failed" | "not_tested";
  functions: Zapret2FunctionSummary[];
  bytes: number;
  medianMs?: number | null;
  transferBytesPerSec?: number | null;
}

export interface Zapret2Tournament {
  runId: string;
  requiredConfirmations: number;
  maxAttempts: number;
  budgetMs: number;
  maxBodyBytes: number;
  elapsedMs: number;
  bytesRead: number;
  stopReason: string;
  candidates: Zapret2CandidateSummary[];
  rounds: Zapret2RoundMeasurement[];
}

export interface Zapret2RoundMeasurement {
  runId: string; profileId: string; fingerprint: string; runtimeGeneration: number | null;
  ordinal: number; phase: string; transport: string; testerVersion: string; execution: string;
  networkBefore: string | null; networkAfter: string | null; comparable: boolean; contextReason: string | null;
  packetCapture?: {
    generation: number; state: string; startupError: string | null;
    capturedPackets: number; queueDrops: number; parseErrors: number; sensorFailed: boolean; capacityEvictions: number;
    flows: Array<{ localAddress: string; remoteAddress: string; synAckObserved: boolean;
      clientHelloBytes: number | null; ackedClientBytes: number | null; outboundPayloadPackets: number;
      repeatedSequencePackets: number; inboundPayloadBytes: number; inboundReset: boolean; inboundFin: boolean;
      tlsServerHelloObserved: boolean; tlsAlertObserved: boolean; conflictingOverlap: boolean }>;
  };
  functions: Array<{
    function: string; label: string; required: boolean;
    outcome: "passed" | "failed" | "inconclusive" | "not_tested" | "cancelled";
    stage: string; proof: string; reason: string | null; target: string;
    remoteAddress: string | null; ipFamily: string | null; elapsedMs: number; bytes: number; httpStatus: number | null;
    transferBytes?: number; transferMs?: number | null;
    connections?: Array<{
      purpose: string; host: string; localAddress: string; remoteAddress: string;
      expectedProfileId: string; observedProfiles: string[]; observedActions: string[]; observedSends?: string[]; status: string;
      http?: { stage: string; startedAfterMs: number; elapsedMs: number; tlsMs: number | null; httpStatus: number | null; bytes: number };
    }>;
    tlsHandshakes?: Array<{
      kind: "large_early_sni" | "large_late_sni";
      handshakeBytes: number; sniOffset: number; hybridKeyBytes: number;
      authenticatedTls: boolean; applicationOk: boolean;
    }>;
  }>;
}

export type Zapret2Scenario = "discord_startup" | "discord_voice" | "youtube_video";
export type Zapret2Observation = "passed" | "failed" | "inconclusive";
export interface Zapret2ScenarioProgress {
  requestId: string;
  trialId: string; runId: string; category: string; transport: string; profileId: string; scenario: Zapret2Scenario;
  phase: "starting" | "step" | "waiting" | "restoring" | "idle";
  stepId: string | null; stepIndex: number; total: number; label: string; instruction: string;
  confirmAfterUnixMs: number; expiresUnixMs: number;
}
export interface Zapret2ScenarioResult {
  requestId: string;
  trialId: string; runId: string; category: string; transport: string; profileId: string; scenario: Zapret2Scenario;
  source: "user_report"; scope: "whole_configuration"; actualTransport: "unverified";
  configurationFingerprint: string; status: "passed" | "failed" | "inconclusive" | "cancelled" | "aborted";
  restored: boolean; contextValid: boolean; recommendationAvailable: boolean;
  failure: string | null; elapsedMs: number; measurement: Zapret2RoundMeasurement;
  historyWarning?: string | null;
}
export interface Zapret2HistoryRecord {
  recordedAtUnixMs: number; kind: "tournament" | "scenario"; subject: string; outcome: string;
  report: Record<string, unknown>;
}

export interface Zapret2PickerProgress {
  category: string;
  transport: string;
  current: number;
  total: number;
  profileId: string;
  label: string;
  phase: "testing" | "screening" | "confirmation" | "composition" | "restoring" | "idle";
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

export type LegacyReliabilityMode = "observe_only" | "assisted" | "automatic";

export type LegacyRecoveryPhase =
  | "preflight"
  | "stopping"
  | "starting"
  | "confirming"
  | "rolling_back"
  | "applied"
  | "process_failed";

export type LegacyRecoveryDisposition =
  | "candidate_applied"
  | "previous_preserved"
  | "rolled_back"
  | "process_failed";

export interface LegacyReliabilityProposal {
  proposalId: number;
  attemptId: number;
  incidentId: number;
  category: string;
  previousConfigId: string;
  candidateConfigId: string;
  /** Backend monotonic time; intentionally not comparable with Date.now(). */
  expiresAtMonotonicMs: number;
}

export type LegacyRecoveryOrigin =
  | { kind: "assisted" }
  | { kind: "automatic"; controlGeneration: number };

export interface LegacyRecoveryAttempt {
  attemptId: number;
  incidentId: number;
  category: string;
  previousConfigId: string;
  candidateConfigId: string;
  origin: LegacyRecoveryOrigin;
  phase: LegacyRecoveryPhase;
  phaseStartedAtMonotonicMs: number;
}

export interface LegacyRecoveryCompletion {
  attemptId: number;
  incidentId: number;
  category: string;
  previousConfigId: string;
  candidateConfigId: string;
  origin: LegacyRecoveryOrigin;
  phase: LegacyRecoveryPhase;
  disposition: LegacyRecoveryDisposition;
  finishedAtMonotonicMs: number;
}

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
  /**
   * UX-only last-known-good memory for the current lane generation. The legacy
   * field name is retained for wire compatibility; it never authorizes a
   * configuration change.
   */
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
  mode: LegacyReliabilityMode;
  phase: LegacyReliabilityPhase;
  activeCategories: string[];
  /** Target applications currently observed; never contains PID or a path. */
  runningApplications: string[];
  sessionId: number | null;
  sensorGeneration: number | null;
  lanes: LegacyReliabilityLaneAssessment[];
  presumedIntent: LegacyReliabilityPresumedIntent;
  proposal: LegacyReliabilityProposal | null;
  activeAttempt: LegacyRecoveryAttempt | null;
  lastCompletion: LegacyRecoveryCompletion | null;
  negativeCooldownCount: number;
  /** Backend-authoritative global latch for future automatic attempts. */
  automaticPaused: boolean;
  /** Remaining backend pacing interval; never compare with the browser clock. */
  automaticPacingRemainingMs: number | null;
  /** User-frozen lanes reconciled by the backend. */
  frozenCategories: string[];
  /** Lanes halted after a terminal recovery failure. */
  haltedCategories: string[];
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

function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return trackBackendRequest(() => isTauri()
    ? tauriInvoke<T>(command, args)
    : invokeBrowserPreview<T>(command, args));
}

export const api = {
  updateCheck: () => invoke<UpdateCheck>("update_check"),
  updateInstall: () => invoke<void>("update_install"),
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
  dpiTest: (category: string, configFile: string, failFast = false) =>
    invoke<DpiTestReport>("dpi_test", { category, configFile, failFast }),
  dpiTestCancel: () => invoke<void>("dpi_test_cancel"),
  dpiEngineList: () => invoke<EngineOption[]>("dpi_engine_list"),
  dpiZapret2Profiles: (categories: string[]) =>
    invoke<Zapret2ProfileDescriptor[]>("dpi_zapret2_profiles", { categories }),
  dpiZapret2Catalog: (categories: string[]) =>
    invoke<Zapret2CatalogProfile[]>("dpi_zapret2_catalog", { categories }),
  dpiZapret2Select: (category: string, transport: string, profileId: string) =>
    invoke<void>("dpi_zapret2_select", { category, transport, profileId }),
  dpiZapret2Pick: (category: string, transport: string, mode: Zapret2PickMode, composition?: {apiProfileId:string;cdnProfileId:string}) =>
    invoke<Zapret2PickerResult>("dpi_zapret2_pick", { category, transport, mode, ...(composition ? {composition} : {}) }),
  dpiZapret2ApplyResult: (runId: string, category: string, transport: string, profileId: string) =>
    invoke<void>("dpi_zapret2_apply_result", { runId, category, transport, profileId }),
  dpiZapret2TestScenario: (requestId: string, runId: string, category: string, transport: string, profileId: string, scenario: Zapret2Scenario) =>
    invoke<Zapret2ScenarioResult>("dpi_zapret2_test_scenario", { requestId, runId, category, transport, profileId, scenario }),
  dpiZapret2ScenarioObserve: (trialId: string, stepId: string, observation: Zapret2Observation) =>
    invoke<void>("dpi_zapret2_scenario_observe", { trialId, stepId, observation }),
  dpiZapret2History: () => invoke<Zapret2HistoryRecord[]>("dpi_zapret2_history"),
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
    invoke<HostsHealthSnapshot>("hosts_install", { provider }),
  hostsRefreshGemini: (preference: GeminiRoutePreference = "auto") =>
    invoke<HostsHealthSnapshot>("hosts_refresh_gemini", { preference }),
  hostsCheck: (maxAgeSeconds: number) =>
    invoke<HostsHealthSnapshot>("hosts_check", { maxAgeSeconds }),
  hostsUninstall: () => invoke<void>("hosts_uninstall"),
  systemDnsStatus: () => invoke<SystemDnsStatus>("system_dns_status"),
  systemDnsRestore: () => invoke<SystemDnsStatus>("system_dns_restore"),
  hostsRestore: (provider: string) =>
    invoke<HostsHealthSnapshot>("hosts_restore", { provider }),

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
  legacyReliabilityApprove: (proposalId: number, attemptId: number) =>
    invoke<void>("legacy_reliability_approve", {
      approval: { proposalId, attemptId },
    }),
};

// ─── События ──────────────────────────────────────────────────────────────

const noopUnlisten: UnlistenFn = () => {};

function listenRuntime<T>(
  event: EventName,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  return isTauri()
    ? listen<T>(event, handler)
    : Promise.resolve(noopUnlisten);
}

export const on = {
  updateProgress: (cb: (progress: UpdateProgress) => void): Promise<UnlistenFn> =>
    listenRuntime<UpdateProgress>("update-progress", (e) => cb(e.payload)),
  log: (cb: (e: LogEvent) => void): Promise<UnlistenFn> =>
    listenRuntime<LogEvent>("log", (e) => cb(e.payload)),
  dpiStatus: (cb: (e: DpiStatus) => void): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<DpiStatus>>("dpi-status", (e) => {
      cb(e.payload.value);
    }),
  dpiStatusVersioned: (
    cb: (section: VersionedSection<DpiStatus>) => void,
  ): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<DpiStatus>>("dpi-status", (e) => cb(e.payload)),
  proxyStatus: (cb: (e: ProxyStatus) => void): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<ProxyStatus>>("proxy-status", (e) => {
      cb(e.payload.value);
    }),
  proxyStatusVersioned: (
    cb: (section: VersionedSection<ProxyStatus>) => void,
  ): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<ProxyStatus>>("proxy-status", (e) => cb(e.payload)),
  brainStatus: (cb: (s: BrainStatus) => void): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<BrainStatus>>("brain://status", (e) => {
      cb(e.payload.value);
    }),
  brainStatusVersioned: (
    cb: (section: VersionedSection<BrainStatus>) => void,
  ): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<BrainStatus>>("brain://status", (e) => cb(e.payload)),
  legacyReliabilityStatus: (
    cb: (status: LegacyReliabilityStatus) => void,
  ): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<LegacyReliabilityStatus>>(
      "legacy-reliability://status",
      (e) => cb(e.payload.value),
    ),
  legacyReliabilityStatusVersioned: (
    cb: (section: VersionedSection<LegacyReliabilityStatus>) => void,
  ): Promise<UnlistenFn> =>
    listenRuntime<VersionedSection<LegacyReliabilityStatus>>(
      "legacy-reliability://status",
      (e) => cb(e.payload),
    ),
  // Rust сообщает о скрытии/показе окна в трей — дополняет Visibility API для
  // паузы анимаций (WebView2 не всегда шлёт visibilitychange на hide()).
  windowVisibility: (cb: (visible: boolean) => void): Promise<UnlistenFn> =>
    listenRuntime<boolean>("window-visibility", (e) => cb(e.payload)),
};

// ─── Runtime snapshots ────────────────────────────────────────────────────
// Legacy snapshot stays available for compatibility; launcher hydration and
// resume use independently versioned bootstrap sections.
export const runtime = {
  listen: listenRuntime,
  snapshot: (): Promise<RuntimeSnapshot> => api.runtimeGetSnapshot(),
  bootstrap: (): Promise<BootstrapSnapshot> => api.bootstrapGetSnapshot(),
};

// ─── Утилиты окна / системы ────────────────────────────────────────────────

export const win = {
  minimize: () =>
    isTauri() ? getCurrentWindow().minimize() : Promise.resolve(),
  toggleMaximize: () =>
    isTauri() ? getCurrentWindow().toggleMaximize() : Promise.resolve(),
  close: () =>
    isTauri() ? getCurrentWindow().close() : Promise.resolve(),
  // Показ/скрытие именно веб-вью: в свёрнутом (iconic) окне композитор WebView2
  // продолжает рисовать кадры — hide() гасит рендер до ~0% CPU, show() возвращает.
  showWebview: () =>
    isTauri() ? getCurrentWebview().show() : Promise.resolve(),
  hideWebview: () =>
    isTauri() ? getCurrentWebview().hide() : Promise.resolve(),
  onFocusChanged: (cb: (focused: boolean) => void): Promise<UnlistenFn> =>
    isTauri()
      ? getCurrentWindow().onFocusChanged(({ payload }) => cb(payload))
      : Promise.resolve(noopUnlisten),
};

export const clipboard = { write: writeText };
export const opener = { open: openUrl };
