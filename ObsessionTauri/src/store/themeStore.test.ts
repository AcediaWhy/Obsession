import { beforeEach, describe, expect, it } from "vitest";
import { useSecretStore } from "./secretStore";
import { THEMES } from "./themeStore";

describe("theme availability", () => {
  beforeEach(() => {
    useSecretStore.setState({ unlocked: [] });
  });

  it("exposes four regular themes and keeps two themes secret", () => {
    expect(THEMES.filter((theme) => !theme.secret).map((theme) => theme.id)).toEqual([
      "aurora",
      "ophanim",
      "japan",
      "midnight",
    ]);
    expect(THEMES.filter((theme) => theme.secret).map((theme) => theme.id)).toEqual([
      "catnap",
      "fallendown",
    ]);
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
  });
});
