import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ check: vi.fn(), install: vi.fn(), toast: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("../lib/tauri", () => ({ api: { updateCheck: mocks.check, updateInstall: mocks.install }, on: {} }));
vi.mock("./toastStore", () => ({ toast: { info: mocks.toast } }));

let now = 0;
let storage: Map<string, string>;
const result = { current_version: __APP_VERSION__, latest_version: __APP_VERSION__, available: false, notes: "",
  release_url: `https://github.com/AcediaWhy/Obsession/releases/tag/v${__APP_VERSION__}` };

beforeEach(() => {
  vi.resetModules(); vi.resetAllMocks();
  now = Date.UTC(2026, 9, 10); storage = new Map();
  vi.spyOn(Date, "now").mockImplementation(() => now);
  vi.stubGlobal("localStorage", { getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value) });
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("update check recovery", () => {
  it("does not count a failed request as a successful daily check and retries after a short backoff", async () => {
    const { useUpdateStore } = await import("./updateStore");
    mocks.check.mockRejectedValueOnce("Сервер недоступен").mockResolvedValue(result);
    await useUpdateStore.getState().check(true);
    expect(storage.has(`obsession.update.last-check.${__APP_VERSION__}`)).toBe(false);
    await useUpdateStore.getState().check(true);
    expect(mocks.check).toHaveBeenCalledTimes(1);
    now += 16 * 60 * 1000;
    await useUpdateStore.getState().check(true);
    expect(mocks.check).toHaveBeenCalledTimes(2);
    expect(useUpdateStore.getState().result).toEqual(result);
    expect(storage.get(`obsession.update.last-check.${__APP_VERSION__}`)).toBe(String(now));
  });
  it("allows a manual retry immediately after a failed automatic request", async () => {
    const { useUpdateStore } = await import("./updateStore");
    mocks.check.mockRejectedValueOnce("Сервер недоступен").mockResolvedValue(result);
    await useUpdateStore.getState().check(true);
    await useUpdateStore.getState().check();
    expect(mocks.check).toHaveBeenCalledTimes(2);
    expect(useUpdateStore.getState().error).toBeNull();
  });
  it("keeps the daily interval after a successful manual check and leaves errors visible for manual failures", async () => {
    const { useUpdateStore } = await import("./updateStore");
    mocks.check.mockResolvedValueOnce(result).mockRejectedValue("Не удалось связаться с сервером обновлений. Попробуй позже.");
    await useUpdateStore.getState().check();
    now += 60 * 60 * 1000;
    await useUpdateStore.getState().check(true);
    expect(mocks.check).toHaveBeenCalledTimes(1);
    await useUpdateStore.getState().check();
    expect(useUpdateStore.getState().phase).toBe("error");
    expect(useUpdateStore.getState().error).toBe("Не удалось связаться с сервером обновлений. Попробуй позже.");
    expect(mocks.toast).not.toHaveBeenCalled();
  });
});
