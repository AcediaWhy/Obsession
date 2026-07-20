import { beforeEach, describe, expect, it } from "vitest";

import type { LegacyReliabilityStatus } from "../lib/tauri";
import {
  INITIAL_LEGACY_RELIABILITY_STATUS,
  useLegacyReliabilityStore,
} from "./legacyReliabilityStore";

function status(
  phase: LegacyReliabilityStatus["phase"],
): LegacyReliabilityStatus {
  const active = phase !== "inactive";
  return {
    mode: "observe_only",
    phase,
    activeCategories: active ? ["discord"] : [],
    sessionId: active ? 17 : null,
    sensorGeneration: active ? 4 : null,
    lanes: active
      ? [
          {
            category: "discord",
            activeConfig: "discord_1.conf",
            laneGeneration: 4,
            phase: "observing",
            classification: "awaiting_evidence",
            confidence: "none",
            evidence: {
              workingFlows: 0,
              workingTargets: 0,
              resetFlows: 0,
              resetTargets: 0,
              blackholeFlows: 0,
              blackholeTargets: 0,
            },
            workingConfirmedRecently: false,
            cooldownUntilMs: null,
          },
        ]
      : [],
    presumedIntent: {
      kind: "wait",
      reason: "awaiting_evidence",
    },
  };
}

describe("legacyReliabilityStore", () => {
  beforeEach(() => {
    useLegacyReliabilityStore.setState({
      revision: -1,
      status: INITIAL_LEGACY_RELIABILITY_STATUS,
    });
  });

  it("starts with an empty journal and a safe wait intent", () => {
    expect(useLegacyReliabilityStore.getState().status).toEqual({
      mode: "observe_only",
      phase: "inactive",
      activeCategories: [],
      sessionId: null,
      sensorGeneration: null,
      lanes: [],
      presumedIntent: {
        kind: "wait",
        reason: "awaiting_evidence",
      },
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
    expect(useLegacyReliabilityStore.getState().status.lanes).toHaveLength(1);
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

  it("preserves the privacy-safe recent Working confirmation projection", () => {
    const recent = status("observing");
    recent.lanes[0].workingConfirmedRecently = true;
    recent.lanes[0].classification = "awaiting_evidence";

    useLegacyReliabilityStore.getState().applyVersionedStatus({
      revision: 1,
      value: recent,
    });

    expect(
      useLegacyReliabilityStore.getState().status.lanes[0]
        .workingConfirmedRecently,
    ).toBe(true);
  });
});
