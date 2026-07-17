import { describe, expect, it } from "vitest";

import type { BootstrapSnapshot, DpiStatus } from "./tauri";

const fixture = {
  schemaVersion: 1,
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
  hosts: {
    revision: 7,
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
  it("keeps every subsystem revision independent", () => {
    expect(fixture.schemaVersion).toBe(1);
    expect([
      fixture.settings.revision,
      fixture.dpi.revision,
      fixture.proxy.revision,
      fixture.brain.revision,
      fixture.adaptive.revision,
      fixture.hosts.revision,
    ]).toEqual([2, 3, 4, 5, 6, 7]);
  });

  it("keeps settings metadata and runtime values in versioned sections", () => {
    expect(fixture.settings.value.settings.ai_provider).toBe("malw");
    expect(fixture.settings.value.elevated).toBe(true);
    expect(fixture.dpi.value.processes).toEqual([]);
    expect(fixture.hosts.value.status).toBe("not_installed");
  });
});
