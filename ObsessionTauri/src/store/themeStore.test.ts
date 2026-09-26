import { beforeEach, describe, expect, it } from "vitest";
import { useSecretStore } from "./secretStore";
import { resolveStoredTheme, THEMES } from "./themeStore";

describe("theme availability", () => {
  beforeEach(() => {
    useSecretStore.setState({ unlocked: [] });
  });

  it("offers Golden Meadow first among five regular themes and keeps three themes secret", () => {
    expect(THEMES.filter((theme) => !theme.secret).map((theme) => theme.id)).toEqual([
      "goldenmeadow",
      "aurora",
      "ophanim",
      "japan",
      "midnight",
    ]);
    expect(THEMES.filter((theme) => theme.secret).map((theme) => theme.id)).toEqual([
      "catnap",
      "fallendown",
      "yanineko",
    ]);
  });

  it("defaults missing, invalid and removed themes to Golden Meadow", () => {
    expect(resolveStoredTheme(null)).toBe("goldenmeadow");
    expect(resolveStoredTheme("unknown-theme")).toBe("goldenmeadow");
    expect(resolveStoredTheme("obsession")).toBe("goldenmeadow");
    expect(resolveStoredTheme("aurora")).toBe("aurora");
    expect(resolveStoredTheme("midnight")).toBe("midnight");
    expect(resolveStoredTheme("yanineko")).toBe("yanineko");
    expect(resolveStoredTheme("goldenmeadow")).toBe("goldenmeadow");
    expect(resolveStoredTheme("quietpond")).toBe("goldenmeadow");
  });

  it("no longer treats Midnight as a secret code", () => {
    expect(useSecretStore.getState().redeem("midnight")).toEqual({ status: "unknown" });
    expect(useSecretStore.getState().redeem("catnap")).toMatchObject({
      status: "unlocked",
      id: "catnap",
    });
    expect(useSecretStore.getState().redeem("fallendown")).toMatchObject({
      status: "unlocked",
      id: "fallendown",
    });
    expect(useSecretStore.getState().redeem("Янинеко")).toMatchObject({
      status: "unlocked",
      id: "yanineko",
    });
    expect(useSecretStore.getState().redeem("yanikasu")).toEqual({ status: "unknown" });
  });
});
