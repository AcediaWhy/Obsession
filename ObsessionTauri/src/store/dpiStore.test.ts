import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("../lib/tauri", () => ({
  api: {},
  runtime: { bootstrap: vi.fn() },
}));

import { useDpiStore } from "./dpiStore";
import type { AppConfig, DpiStatus } from "../lib/tauri";

function status(partial: Partial<DpiStatus>): DpiStatus {
  return { active: false, processes: [], started_at: null, ...partial };
}

describe("dpiStore.initialize: снятая с поддержки категория", () => {
  it.each([
    { stored: ["atrisk", "gaming"], expected: ["gaming"] },
    { stored: ["atrisk"], expected: ["discord"] },
  ])("не запускает скрытую группу из старого выбора $stored", ({ stored, expected }) => {
    const config: AppConfig = {
      categories: ["atrisk", "discord", "gaming"],
      configs: {
        atrisk: ["atrisk_1.conf"], discord: ["discord_1.conf"], gaming: ["gaming_1.conf"],
      },
      lists: [],
    };
    useDpiStore.getState().initialize(config, {
      minimize_to_tray: true,
      start_minimized: false,
      dpi_engine: "legacy",
      selected_categories: stored,
      selected_configs: { atrisk: "atrisk_1.conf", gaming: "gaming_1.conf" },
      zapret2_selected_categories: ["discord"],
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
      zapret2_level: 0,
      adaptive_strategy_enabled: false,
      adaptive_search_mode: "balanced",
    });
    expect(useDpiStore.getState().selectedCategories).toEqual(expected);
    expect(useDpiStore.getState().categorySelections.legacy).toEqual(expected);
    expect(useDpiStore.getState().selectedConfigs).not.toHaveProperty("atrisk");
    expect(useDpiStore.getState().selectedConfigs.gaming).toBe("gaming_1.conf");
  });
});

describe("dpiStore.applyStatus", () => {
  beforeEach(() => {
    useDpiStore.setState({
      revision: -1,
      active: false,
      processes: [],
      startedAt: null,
      transitioning: false,
    });
  });

  it("конвертирует started_at из Unix-сек в мс", () => {
    useDpiStore.getState().applyStatus(status({ active: true, started_at: 1_700_000 }));
    expect(useDpiStore.getState().startedAt).toBe(1_700_000_000);
    expect(useDpiStore.getState().active).toBe(true);
  });

  it("started_at=null → startedAt=null (обход выключен)", () => {
    useDpiStore.setState({ startedAt: 123456 });
    useDpiStore.getState().applyStatus(status({ active: false, started_at: null }));
    expect(useDpiStore.getState().startedAt).toBeNull();
  });

  it("НЕ трогает transitioning — им владеют start/stop", () => {
    useDpiStore.setState({ transitioning: true });
    useDpiStore.getState().applyStatus(status({ active: true, started_at: 1 }));
    expect(useDpiStore.getState().transitioning).toBe(true);
  });

  it("не даёт старому snapshot перетереть более свежее событие", () => {
    expect(
      useDpiStore.getState().applyVersionedStatus({
        revision: 2,
        value: status({ active: true, started_at: 10 }),
      }),
    ).toBe(true);
    expect(
      useDpiStore.getState().applyVersionedStatus({
        revision: 1,
        value: status({ active: false, started_at: null }),
      }),
    ).toBe(false);
    expect(useDpiStore.getState().revision).toBe(2);
    expect(useDpiStore.getState().active).toBe(true);
    expect(useDpiStore.getState().startedAt).toBe(10_000);
  });
});
