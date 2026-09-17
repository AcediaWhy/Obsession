import { beforeEach, describe, expect, it } from "vitest";
import { useSecretStore } from "./secretStore";
import { resolveStoredTheme, THEMES } from "./themeStore";

describe("theme availability", () => {
  beforeEach(() => {
    useSecretStore.setState({ unlocked: [] });
  });

  it("offers Golden Meadow first among six regular themes and keeps three themes secret", () => {
    expect(THEMES.filter((theme) => !theme.secret).map((theme) => theme.id)).toEqual([
      "goldenmeadow",
      "obsession",
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

  it("defaults only missing or invalid values to Obsession", () => {
    expect(resolveStoredTheme(null)).toBe("obsession");
    expect(resolveStoredTheme("unknown-theme")).toBe("obsession");
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
