import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { api, type BootstrapSnapshot, type DpiStatus } from "./tauri";

const fixture = {
  schemaVersion: 4,
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
        legacy_reliability_mode: "observe_only",
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
    expect(fixture.schemaVersion).toBe(4);
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
    expect(fixture.dpi.value.processes).toEqual([]);
    expect(fixture.legacyReliability.value).toEqual({
      mode: "assisted",
      phase: "observing",
      activeCategories: ["discord"],
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
});
