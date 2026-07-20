import { beforeEach, describe, expect, it } from "vitest";

import type { LegacyReliabilityStatus } from "../lib/tauri";
import { useLegacyReliabilityStore } from "./legacyReliabilityStore";

function status(
  phase: LegacyReliabilityStatus["phase"],
): LegacyReliabilityStatus {
  return {
    mode: "observe_only",
    phase,
    activeCategories: phase === "inactive" ? [] : ["discord"],
    sessionId: phase === "inactive" ? null : 17,
    sensorGeneration: phase === "observing" ? 4 : null,
  };
}

describe("legacyReliabilityStore", () => {
  beforeEach(() => {
    useLegacyReliabilityStore.setState({
      revision: -1,
      status: status("inactive"),
    });
  });

  it("keeps a newer listener event when an older snapshot arrives", () => {
    const observing = status("observing");

    expect(
      useLegacyReliabilityStore.getState().applyVersionedStatus({
        revision: 2,
        value: observing,
      }),
    ).toBe(true);
    expect(
      useLegacyReliabilityStore.getState().applyVersionedStatus({
        revision: 1,
        value: status("starting"),
      }),
    ).toBe(false);

    expect(useLegacyReliabilityStore.getState()).toMatchObject({
      revision: 2,
      status: observing,
    });
  });

  it("accepts only strictly newer revisions", () => {
    useLegacyReliabilityStore.getState().applyVersionedStatus({
      revision: 3,
      value: status("degraded"),
    });

    expect(
      useLegacyReliabilityStore.getState().applyVersionedStatus({
        revision: 3,
        value: status("blind"),
      }),
    ).toBe(false);
    expect(
      useLegacyReliabilityStore.getState().applyVersionedStatus({
        revision: 4,
        value: status("blind"),
      }),
    ).toBe(true);
    expect(useLegacyReliabilityStore.getState().status.phase).toBe("blind");
  });
});
