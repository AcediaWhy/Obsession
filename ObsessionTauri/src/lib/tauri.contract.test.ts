import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { api, type BootstrapSnapshot, type DpiStatus } from "./tauri";

const fixture = {
  schemaVersion: 8,
  settings: {
    revision: 2,
    value: {
      settings: {
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
        legacy_reliability_enabled: true,
        legacy_reliability_mode: "observe_only",
        legacy_automatic_paused: true,
        legacy_reliability_frozen_categories: [],
        reduce_motion: false,
        hotkey_toggle: "Ctrl+Shift+KeyO",
        lan_publish_secs: 0,
        dpi_engine: "zapret2",
        zapret2_level: 0,
        adaptive_strategy_enabled: true,
        adaptive_search_mode: "balanced",
      },
      elevated: true,
      autostart: false,
      protectedRuntime: {
        serviceAvailable: true,
        serviceVersion: "1.2.3",
        dpi: true,
        zapret2: true,
        adaptiveZapret2: true,
        eyesEvents: true,
        legacyReliabilityControls: true,
        legacyReliability: true,
        hosts: false,
        proxyLanFirewall: false,
      },
      protectedRuntimeAvailable: false,
      protectedDpiAvailable: true,
      protectedLegacyReliabilityAvailable: false,
    },
  },
  dpi: {
    revision: 3,
    value: {
      active: false,
      processes: [],
      started_at: null,
    } as DpiStatus,
  },
  proxy: {
    revision: 4,
    value: {
      running: false,
      link: "",
      lan_link: null,
      lan_published: false,
      lan_expiry_unix: null,
    },
  },
  brain: { revision: 5, value: null },
  adaptive: { revision: 6, value: null },
  legacyReliability: {
    revision: 7,
    value: {
      mode: "assisted",
      phase: "observing",
      activeCategories: ["discord"],
      runningApplications: ["discord"],
      sessionId: 42,
      sensorGeneration: 9,
      lanes: [
        {
          category: "discord",
          activeConfig: "discord_1.conf",
          laneGeneration: 9,
          phase: "suspect",
          classification: "dpi_suspected",
          confidence: "high",
          evidence: {
            workingFlows: 0,
            workingTargets: 0,
            resetFlows: 3,
            resetTargets: 2,
            blackholeFlows: 0,
            blackholeTargets: 0,
          },
          workingConfirmedRecently: false,
          cooldownUntilMs: null,
        },
      ],
      presumedIntent: {
        kind: "switch_lane",
        category: "discord",
        candidateConfig: "discord_2.conf",
        reason: "dpi_suspected",
      },
      proposal: {
        proposalId: 3,
        attemptId: 5,
        incidentId: 2,
        category: "discord",
        previousConfigId: "discord_1.conf",
        candidateConfigId: "discord_2.conf",
        expiresAtMonotonicMs: 45_000,
      },
      activeAttempt: null,
      lastCompletion: null,
      negativeCooldownCount: 1,
      automaticPaused: true,
      automaticPacingRemainingMs: null,
      frozenCategories: [],
      haltedCategories: [],
    },
  },
  hosts: {
    revision: 8,
    value: {
      provider: "malw",
      status: "not_installed",
      local_version: "",
      remote_version: "",
      rollback_available: false,
    },
  },
} satisfies BootstrapSnapshot;

describe("BootstrapSnapshot contract", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("keeps every subsystem revision independent", () => {
    expect(fixture.schemaVersion).toBe(8);
    expect([
      fixture.settings.revision,
      fixture.dpi.revision,
      fixture.proxy.revision,
      fixture.brain.revision,
      fixture.adaptive.revision,
      fixture.legacyReliability.revision,
      fixture.hosts.revision,
    ]).toEqual([2, 3, 4, 5, 6, 7, 8]);
  });

  it("keeps settings metadata and runtime values in versioned sections", () => {
    expect(fixture.settings.value.settings.ai_provider).toBe("malw");
    expect(fixture.settings.value.elevated).toBe(true);
    expect(fixture.settings.value.protectedRuntime).toEqual({
      serviceAvailable: true,
      serviceVersion: "1.2.3",
      dpi: true,
      zapret2: true,
      adaptiveZapret2: true,
      eyesEvents: true,
      legacyReliabilityControls: true,
      legacyReliability: true,
      hosts: false,
      proxyLanFirewall: false,
    });
    expect(fixture.settings.value.protectedRuntimeAvailable).toBe(false);
    expect(fixture.settings.value.protectedDpiAvailable).toBe(true);
    expect(fixture.settings.value.protectedLegacyReliabilityAvailable).toBe(false);
    expect(fixture.dpi.value.processes).toEqual([]);
    expect(fixture.legacyReliability.value).toEqual({
      mode: "assisted",
      phase: "observing",
      activeCategories: ["discord"],
      runningApplications: ["discord"],
      sessionId: 42,
      sensorGeneration: 9,
      lanes: [
        {
          category: "discord",
          activeConfig: "discord_1.conf",
          laneGeneration: 9,
          phase: "suspect",
          classification: "dpi_suspected",
          confidence: "high",
          evidence: {
            workingFlows: 0,
            workingTargets: 0,
            resetFlows: 3,
            resetTargets: 2,
            blackholeFlows: 0,
            blackholeTargets: 0,
          },
          workingConfirmedRecently: false,
          cooldownUntilMs: null,
        },
      ],
      presumedIntent: {
        kind: "switch_lane",
        category: "discord",
        candidateConfig: "discord_2.conf",
        reason: "dpi_suspected",
      },
      proposal: {
        proposalId: 3,
        attemptId: 5,
        incidentId: 2,
        category: "discord",
        previousConfigId: "discord_1.conf",
        candidateConfigId: "discord_2.conf",
        expiresAtMonotonicMs: 45_000,
      },
      activeAttempt: null,
      lastCompletion: null,
      negativeCooldownCount: 1,
      automaticPaused: true,
      automaticPacingRemainingMs: null,
      frozenCategories: [],
      haltedCategories: [],
    });
    expect(fixture.hosts.value.status).toBe("not_installed");
  });

  it("sends only the backend-owned assisted approval token", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await api.legacyReliabilityApprove(3, 5);

    expect(invoke).toHaveBeenCalledWith("legacy_reliability_approve", {
      approval: { proposalId: 3, attemptId: 5 },
    });
  });

  it("sends Automatic controls through atomic settings patches", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await api.updateSettings({ legacy_reliability_enabled: false });
    await api.updateSettings({ legacy_reliability_mode: "automatic" });
    await api.updateSettings({ legacy_automatic_paused: true });
    await api.updateSettings({
      legacy_reliability_frozen_categories: ["discord"],
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "update_settings", {
      patch: { legacy_reliability_enabled: false },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "update_settings", {
      patch: { legacy_reliability_mode: "automatic" },
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "update_settings", {
      patch: { legacy_automatic_paused: true },
    });
    expect(invoke).toHaveBeenNthCalledWith(4, "update_settings", {
      patch: { legacy_reliability_frozen_categories: ["discord"] },
    });
  });

  it("keeps route checks read-only and bounded by a cache age", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await api.hostsCheck(900);

    expect(invoke).toHaveBeenCalledWith("hosts_check", {
      maxAgeSeconds: 900,
    });
  });
});
