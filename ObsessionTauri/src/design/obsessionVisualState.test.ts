import { describe, expect, it } from "vitest";
import {
  deriveObsessionVisualPhase,
  obsessionFocusForScreen,
  obsessionPhaseValue,
  type ObsessionVisualSignals,
} from "./obsessionVisualState";

function signals(
  patch: Partial<ObsessionVisualSignals> = {},
): ObsessionVisualSignals {
  return {
    dpi: { active: false, transitioning: false, testing: false, error: "" },
    proxy: { running: false, transitioning: false, error: "" },
    adaptive: { busy: false, error: "", phase: null },
    brain: { phase: null },
    reliability: {
      phase: "inactive",
      activeAttemptPhase: null,
      haltedCategories: [],
    },
    ...patch,
  };
}

describe("deriveObsessionVisualPhase", () => {
  it("projects idle, focused, engaging and scanning states", () => {
    expect(deriveObsessionVisualPhase(signals())).toBe("idle");
    expect(
      deriveObsessionVisualPhase(
        signals({ dpi: { active: true, transitioning: false, testing: false, error: "" } }),
      ),
    ).toBe("focused");
    expect(
      deriveObsessionVisualPhase(
        signals({ proxy: { running: true, transitioning: true, error: "" } }),
      ),
    ).toBe("engaging");
    expect(
      deriveObsessionVisualPhase(
        signals({ adaptive: { busy: false, error: "", phase: "candidate_probe" } }),
      ),
    ).toBe("scanning");
  });

  it("uses the documented fault to scanning to engaging priority", () => {
    expect(
      deriveObsessionVisualPhase(
        signals({
          dpi: { active: true, transitioning: true, testing: true, error: "runtime failed" },
        }),
      ),
    ).toBe("fault");
    expect(
      deriveObsessionVisualPhase(
        signals({
          dpi: { active: true, transitioning: true, testing: true, error: "" },
        }),
      ),
    ).toBe("scanning");
  });

  it("treats terminal recovery and sensor states as faults", () => {
    expect(
      deriveObsessionVisualPhase(
        signals({
          reliability: {
            phase: "blind",
            activeAttemptPhase: null,
            haltedCategories: [],
          },
        }),
      ),
    ).toBe("fault");
    expect(
      deriveObsessionVisualPhase(
        signals({ brain: { phase: "exhausted" } }),
      ),
    ).toBe("fault");
  });
});

describe("Obsession scene helpers", () => {
  it("uses Overview's open lower-right field and keeps dense screens upper-right", () => {
    expect(obsessionFocusForScreen("overview")).toEqual({ x: 0.82, y: 0.78 });
    for (const screen of ["dpi", "ai", "telegram", "lists", "profiles", "settings"]) {
      const focus = obsessionFocusForScreen(screen);
      expect(focus.x).toBeGreaterThanOrEqual(0.7);
      expect(focus.x).toBeLessThan(0.9);
      expect(focus.y).toBeGreaterThan(0.15);
      expect(focus.y).toBeLessThan(0.35);
    }
  });

  it("maps every phase to a stable shader value", () => {
    expect(["idle", "engaging", "scanning", "focused", "fault"].map((phase) =>
      obsessionPhaseValue(phase as Parameters<typeof obsessionPhaseValue>[0]),
    )).toEqual([0, 1, 2, 3, 4]);
  });
});
