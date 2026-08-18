import type {
  BootstrapSnapshot,
  HostsHealthSnapshot,
  Settings,
} from "./tauri";

const previewCapabilities = {
  serviceAvailable: false,
  serviceVersion: null,
  dpi: false,
  zapret2: false,
  adaptiveZapret2: false,
  eyesEvents: false,
  legacyReliabilityControls: false,
  legacyReliability: false,
  hosts: false,
  proxyLanFirewall: false,
};

let previewSettings: Settings = {
  minimize_to_tray: true,
  start_minimized: false,
  selected_categories: ["discord"],
  zapret2_selected_categories: ["discord"],
  selected_configs: {},
  proxy_port: 1443,
  fake_tls_domain: "",
  ai_provider: "malw",
  has_completed_onboarding: true,
  auto_recovery: false,
  legacy_reliability_migration_version: 1,
  legacy_reliability_enabled: false,
  legacy_reliability_mode: "observe_only",
  legacy_automatic_paused: false,
  legacy_reliability_frozen_categories: [],
  reduce_motion: false,
  hotkey_toggle: "Ctrl+Shift+KeyO",
  lan_publish_secs: 0,
  dpi_engine: "legacy",
  zapret2_level: 0,
  adaptive_strategy_enabled: false,
  adaptive_search_mode: "balanced",
};

function bootstrapSnapshot(): BootstrapSnapshot {
  return {
    schemaVersion: 8,
    settings: {
      revision: 0,
      value: {
        settings: { ...previewSettings },
        elevated: false,
        autostart: false,
        protectedRuntime: previewCapabilities,
        protectedRuntimeAvailable: false,
        protectedDpiAvailable: false,
        protectedLegacyReliabilityAvailable: false,
      },
    },
    dpi: {
      revision: 0,
      value: { active: false, processes: [], started_at: null },
    },
    proxy: {
      revision: 0,
      value: {
        running: false,
        link: "",
        lan_link: null,
        lan_published: false,
        lan_expiry_unix: null,
      },
    },
    brain: { revision: 0, value: null },
    adaptive: { revision: 0, value: null },
    legacyReliability: {
      revision: 0,
      value: {
        mode: "observe_only",
        phase: "inactive",
        activeCategories: [],
        runningApplications: [],
        sessionId: 0,
        sensorGeneration: 0,
        lanes: [],
        presumedIntent: { kind: "wait", reason: "awaiting_evidence" },
        proposal: null,
        activeAttempt: null,
        lastCompletion: null,
        negativeCooldownCount: 0,
        automaticPaused: false,
        automaticPacingRemainingMs: null,
        frozenCategories: [],
        haltedCategories: [],
      },
    },
    hosts: {
      revision: 0,
      value: {
        provider: "malw",
        status: "not_installed",
        local_version: "",
        remote_version: "",
        rollback_available: false,
      },
    },
  };
}

const previewHostsHealth: HostsHealthSnapshot = {
  preferredProvider: "malw",
  installed: false,
  checkedAtUnix: null,
  repairRecommended: false,
  services: ["chatgpt", "claude", "gemini"].map((service) => ({
    service: service as "chatgpt" | "claude" | "gemini",
    health: "unchecked",
    route: "direct",
    provider: null,
    reason: null,
  })),
};

export function invokeBrowserPreview<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  let value: unknown;
  switch (command) {
    case "bootstrap_get_snapshot":
      value = bootstrapSnapshot();
      break;
    case "get_config":
      value = {
        categories: ["discord"],
        configs: { discord: [] },
        lists: [],
      };
      break;
    case "proxy_available":
    case "get_autostart":
    case "is_elevated":
      value = false;
      break;
    case "get_settings":
      value = { ...previewSettings };
      break;
    case "update_settings":
      previewSettings = {
        ...previewSettings,
        ...((args?.patch as Partial<Settings> | undefined) ?? {}),
      };
      value = { ...previewSettings };
      break;
    case "get_netcache_stats":
      value = {};
      break;
    case "diagnose":
      value = [
        { name: "Discord", url: "https://discord.com", ok: true, ms: 42 },
        { name: "YouTube", url: "https://youtube.com", ok: true, ms: 67 },
        { name: "Telegram", url: "https://telegram.org", ok: true, ms: 51 },
      ];
      break;
    case "dpi_engine_list":
    case "dpi_zapret2_profiles":
    case "dpi_detect_orphaned":
    case "lists_all":
    case "get_profiles":
      value = [];
      break;
    case "hosts_check":
      value = previewHostsHealth;
      break;
    default:
      return Promise.reject(
        new Error(`Tauri command is unavailable in browser preview: ${command}`),
      );
  }
  return Promise.resolve(value as T);
}
