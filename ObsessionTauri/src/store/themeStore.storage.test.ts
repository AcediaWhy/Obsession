import { afterEach, describe, expect, it, vi } from "vitest";

describe("Yani Neko theme persistence", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("restores Yani Neko and writes later selections through the existing key", async () => {
    const storage = {
      getItem: vi.fn((key: string) => key === "obsession.theme" ? "yanineko" : null),
      setItem: vi.fn(),
    };
    vi.stubGlobal("localStorage", storage);
    vi.resetModules();
    const { useThemeStore } = await import("./themeStore");

    expect(useThemeStore.getState().theme).toBe("yanineko");
    useThemeStore.getState().setTheme("aurora");
    expect(storage.setItem).toHaveBeenCalledWith("obsession.theme", "aurora");
  });
});
