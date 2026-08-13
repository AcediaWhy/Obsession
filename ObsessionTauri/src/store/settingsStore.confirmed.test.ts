import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  getSettings: vi.fn(),
}));

vi.mock("../lib/tauri", () => ({
  api: {
    updateSettings: mocks.updateSettings,
    getSettings: mocks.getSettings,
  },
}));

import type { Settings } from "../lib/tauri";
import { useSettingsStore } from "./settingsStore";

const initialSettings: Settings = {
  minimize_to_tray: true,
  start_minimized: false,
  selected_categories: ["discord"],
  zapret2_selected_categories: ["discord"],
  selected_configs: {},
  proxy_port: 1443,
  fake_tls_domain: "",
  ai_provider: "malw",
  has_completed_onboarding: false,
  auto_recovery: false,
  legacy_reliability_migration_version: 1,
  legacy_reliability_enabled: true,
  legacy_reliability_mode: "observe_only",
  legacy_automatic_paused: true,
  legacy_reliability_frozen_categories: [],
  reduce_motion: false,
  hotkey_toggle: "Ctrl+Shift+KeyO",
  lan_publish_secs: 0,
  dpi_engine: "legacy",
  zapret2_level: 0,
  adaptive_strategy_enabled: false,
  adaptive_search_mode: "balanced",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("settingsStore.patchConfirmed", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      revision: 1,
      loaded: true,
      saving: false,
      saved: false,
      settings: initialSettings,
      elevated: false,
      protectedRuntimeAvailable: false,
      protectedDpiAvailable: false,
      protectedLegacyReliabilityAvailable: false,
      autostart: false,
      error: "",
    });
  });

  it("does not publish the onboarding flag before Rust confirms persistence", async () => {
    const request = deferred<Settings>();
    mocks.updateSettings.mockReturnValueOnce(request.promise);

    const pending = useSettingsStore
      .getState()
      .patchConfirmed({ has_completed_onboarding: true });

    expect(useSettingsStore.getState().settings?.has_completed_onboarding).toBe(false);
    expect(useSettingsStore.getState().saving).toBe(true);

    request.resolve({ ...initialSettings, has_completed_onboarding: true });

    await expect(pending).resolves.toBe(true);
    expect(useSettingsStore.getState().settings?.has_completed_onboarding).toBe(true);
    expect(useSettingsStore.getState().saving).toBe(false);
  });

  it("keeps the authoritative flag false and returns retryable failure", async () => {
    mocks.updateSettings.mockRejectedValueOnce(new Error("disk locked"));
    mocks.getSettings.mockResolvedValueOnce(initialSettings);

    await expect(
      useSettingsStore
        .getState()
        .patchConfirmed({ has_completed_onboarding: true }),
    ).resolves.toBe(false);

    expect(useSettingsStore.getState().settings?.has_completed_onboarding).toBe(false);
    expect(useSettingsStore.getState().saving).toBe(false);
    expect(useSettingsStore.getState().error).toContain("disk locked");
  });
});
