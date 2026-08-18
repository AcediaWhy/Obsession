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

import { api, on, win, type LogEvent } from "./tauri";

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
});
