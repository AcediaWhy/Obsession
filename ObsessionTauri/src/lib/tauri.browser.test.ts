import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(),
  listen: vi.fn(),
  getCurrentWindow: vi.fn(),
  getCurrentWebview: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  isTauri: mocks.isTauri,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: mocks.getCurrentWindow,
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: mocks.getCurrentWebview,
}));

import { api, on, runtime, win, type LogEvent } from "./tauri";

describe("Tauri bridge outside the native runtime", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.isTauri.mockReturnValue(false);
  });

  it("returns inert listener cleanups without touching Tauri globals", async () => {
    const unlistenLog = await on.log(vi.fn());
    const unlistenVisibility = await on.windowVisibility(vi.fn());
    const unlistenFocus = await win.onFocusChanged(vi.fn());

    expect(mocks.listen).not.toHaveBeenCalled();
    expect(mocks.getCurrentWindow).not.toHaveBeenCalled();
    expect(() => unlistenLog()).not.toThrow();
    expect(() => unlistenVisibility()).not.toThrow();
    expect(() => unlistenFocus()).not.toThrow();
  });

  it("keeps window controls inert in a browser preview", async () => {
    await Promise.all([
      win.minimize(),
      win.toggleMaximize(),
      win.close(),
      win.showWebview(),
      win.hideWebview(),
    ]);

    expect(mocks.getCurrentWindow).not.toHaveBeenCalled();
    expect(mocks.getCurrentWebview).not.toHaveBeenCalled();
  });

  it("hydrates a read-only preview without invoking the native backend", async () => {
    const snapshot = await api.bootstrapGetSnapshot();

    expect(snapshot.settings.value.settings.ai_provider).toBe("malw");
    expect(snapshot.settings.value.settings.has_completed_onboarding).toBe(true);
    expect(snapshot.dpi.value.active).toBe(false);
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("keeps the Zapret2 catalog read-only in preview", async () => {
    await expect(api.dpiZapret2Catalog(["youtube_twitch"])).resolves.toEqual([]);
    await expect(api.dpiZapret2Select("youtube_twitch", "tls", "youtube-other")).rejects.toThrow();
    await expect(api.dpiZapret2ApplyResult("run", "youtube_twitch", "tls", "youtube-other")).rejects.toThrow();
    await expect(api.dpiZapret2History()).resolves.toEqual([]);
    await expect(api.dpiZapret2TestScenario("request", "run", "youtube_twitch", "tls", "youtube-other", "youtube_video")).rejects.toThrow();
    await expect(api.dpiZapret2ScenarioObserve("trial", "playback", "passed")).rejects.toThrow();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});

describe("Tauri bridge inside the native runtime", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.isTauri.mockReturnValue(true);
  });

  it("preserves native event payload mapping", async () => {
    const unlisten = vi.fn();
    const callback = vi.fn();
    const event: LogEvent = {
      level: "info",
      source: "test",
      message: "ready",
      ts: "2026-08-17T00:00:00.000Z",
    };
    mocks.listen.mockResolvedValue(unlisten);

    await expect(on.log(callback)).resolves.toBe(unlisten);
    const handler = mocks.listen.mock.calls[0][1];
    handler({ payload: event });

    expect(mocks.listen).toHaveBeenCalledWith("log", expect.any(Function));
    expect(callback).toHaveBeenCalledWith(event);
  });

  it("preserves native focus subscription", async () => {
    const unlisten = vi.fn();
    const callback = vi.fn();
    const onFocusChanged = vi.fn().mockResolvedValue(unlisten);
    mocks.getCurrentWindow.mockReturnValue({ onFocusChanged });

    await expect(win.onFocusChanged(callback)).resolves.toBe(unlisten);
    const handler = onFocusChanged.mock.calls[0][0];
    handler({ payload: true });

    expect(callback).toHaveBeenCalledWith(true);
  });

  it("passes independent profile and picker arguments to the backend", async () => {
    await api.dpiZapret2Catalog(["discord", "youtube_twitch"]);
    await api.dpiZapret2Select("youtube_twitch", "quic", "youtube-quic");
    await api.dpiZapret2Pick("discord", "tls", "deep");
    await api.dpiZapret2ApplyResult("tournament-1", "discord", "tls", "discord-profile");
    await api.dpiZapret2TestScenario("request", "tournament-1", "discord", "tls", "discord-profile", "discord_startup");
    await api.dpiZapret2ScenarioObserve("trial", "channels", "inconclusive");
    expect(mocks.invoke.mock.calls).toEqual([
      ["dpi_zapret2_catalog", { categories: ["discord", "youtube_twitch"] }],
      ["dpi_zapret2_select", { category: "youtube_twitch", transport: "quic", profileId: "youtube-quic" }],
      ["dpi_zapret2_pick", { category: "discord", transport: "tls", mode: "deep" }],
      ["dpi_zapret2_apply_result", { runId: "tournament-1", category: "discord", transport: "tls", profileId: "discord-profile" }],
      ["dpi_zapret2_test_scenario", { requestId: "request", runId: "tournament-1", category: "discord", transport: "tls", profileId: "discord-profile", scenario: "discord_startup" }],
      ["dpi_zapret2_scenario_observe", { trialId: "trial", stepId: "channels", observation: "inconclusive" }],
    ]);
  });

  it("subscribes to picker progress with a disposable native listener", async () => {
    const cleanup = vi.fn();
    const callback = vi.fn();
    mocks.listen.mockResolvedValue(cleanup);
    await expect(runtime.listen("zapret2-picker-progress", callback)).resolves.toBe(cleanup);
    expect(mocks.listen).toHaveBeenCalledWith("zapret2-picker-progress", callback);
  });
});
