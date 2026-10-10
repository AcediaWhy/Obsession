import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });

async function load(seen: boolean) {
  const storage = { getItem: vi.fn(() => seen ? "1" : null), setItem: vi.fn() };
  vi.stubGlobal("localStorage", storage);
  vi.resetModules();
  const { useStartupIntro } = await import("./startupIntro");
  return { store: useStartupIntro, storage };
}

describe("startup intro session", () => {
  it("keeps the first full scene eligible until it is completed or skipped", async () => {
    const { store, storage } = await load(false);
    expect(store.getState().request).toMatchObject({ mode: "full", replay: false });
    expect(storage.setItem).not.toHaveBeenCalled();
    store.getState().finish(store.getState().request!.id);
    expect(store.getState().request).toBeNull();
    expect(storage.setItem).toHaveBeenCalledWith("obsession.intro.seen", "1");
  });

  it("uses a short return and lets the user replay the full scene", async () => {
    const { store } = await load(true);
    expect(store.getState().request?.mode).toBe("quick");
    store.getState().finish(store.getState().request!.id);
    store.getState().replay();
    expect(store.getState().request).toMatchObject({ mode: "full", replay: true });
  });

  it("ignores old timers after a replay replaces their sequence", async () => {
    const { store } = await load(false);
    const oldId = store.getState().request!.id;
    store.getState().replay();
    const current = store.getState().request;
    store.getState().arrive(oldId);
    store.getState().finish(oldId);
    expect(store.getState().request).toBe(current);
    expect(store.getState().arriving).toBe(false);
    store.getState().arrive(current!.id);
    expect(store.getState().arriving).toBe(true);
  });

  it("does not consume the first scene when interrupted by a hidden window", async () => {
    const { store, storage } = await load(false);
    store.getState().finish(store.getState().request!.id, false);
    expect(store.getState().request).toBeNull();
    expect(storage.setItem).not.toHaveBeenCalled();
  });

  it("still reaches an interactive app when storage is unavailable", async () => {
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("denied"); }, setItem: () => { throw new Error("denied"); } });
    vi.resetModules();
    const { useStartupIntro } = await import("./startupIntro");
    expect(() => useStartupIntro.getState().finish(useStartupIntro.getState().request!.id)).not.toThrow();
    expect(useStartupIntro.getState().request).toBeNull();
  });
});
