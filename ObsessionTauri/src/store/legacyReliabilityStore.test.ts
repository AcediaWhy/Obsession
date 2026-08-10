import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/tauri", () => ({
  api: {
    legacyReliabilityApprove: vi.fn(),
  },
}));

import { api, type LegacyReliabilityStatus } from "../lib/tauri";
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
    runningApplications: [],
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
    proposal: null,
    activeAttempt: null,
    lastCompletion: null,
    negativeCooldownCount: 0,
    automaticPaused: true,
    automaticPacingRemainingMs: null,
    frozenCategories: [],
    haltedCategories: [],
  };
}

describe("legacyReliabilityStore", () => {
  beforeEach(() => {
    useLegacyReliabilityStore.setState({
      revision: -1,
      status: INITIAL_LEGACY_RELIABILITY_STATUS,
      approvalPending: false,
      approvalError: "",
    });
    vi.mocked(api.legacyReliabilityApprove).mockReset();
  });

  it("starts with an empty journal and a safe wait intent", () => {
    expect(useLegacyReliabilityStore.getState().status).toEqual({
      mode: "observe_only",
      phase: "inactive",
      activeCategories: [],
      runningApplications: [],
      sessionId: null,
      sensorGeneration: null,
      lanes: [],
      presumedIntent: {
        kind: "wait",
        reason: "awaiting_evidence",
      },
      proposal: null,
      activeAttempt: null,
      lastCompletion: null,
      negativeCooldownCount: 0,
      automaticPaused: true,
      automaticPacingRemainingMs: null,
      frozenCategories: [],
      haltedCategories: [],
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

  it("preserves backend-authoritative Automatic controls", () => {
    const automatic = status("observing");
    automatic.mode = "automatic";
    automatic.automaticPaused = true;
    automatic.automaticPacingRemainingMs = 17_500;
    automatic.frozenCategories = ["discord"];
    automatic.haltedCategories = ["youtube_twitch"];

    useLegacyReliabilityStore.getState().applyVersionedStatus({
      revision: 1,
      value: automatic,
    });

    expect(useLegacyReliabilityStore.getState().status).toMatchObject({
      mode: "automatic",
      automaticPaused: true,
      automaticPacingRemainingMs: 17_500,
      frozenCategories: ["discord"],
      haltedCategories: ["youtube_twitch"],
    });
  });

  it("submits only the opaque proposal and attempt identifiers", async () => {
    vi.mocked(api.legacyReliabilityApprove).mockResolvedValue(undefined);

    const approved = await useLegacyReliabilityStore
      .getState()
      .approveProposal({ proposalId: 31, attemptId: 47 });

    expect(approved).toBe(true);
    expect(api.legacyReliabilityApprove).toHaveBeenCalledWith(31, 47);
    expect(useLegacyReliabilityStore.getState()).toMatchObject({
      approvalPending: false,
      approvalError: "",
    });
  });

  it("blocks duplicate approval while a request is in flight", async () => {
    let finish!: () => void;
    vi.mocked(api.legacyReliabilityApprove).mockImplementation(
      () => new Promise<void>((resolve) => (finish = resolve)),
    );

    const first = useLegacyReliabilityStore
      .getState()
      .approveProposal({ proposalId: 31, attemptId: 47 });
    expect(useLegacyReliabilityStore.getState().approvalPending).toBe(true);

    await expect(
      useLegacyReliabilityStore
        .getState()
        .approveProposal({ proposalId: 31, attemptId: 47 }),
    ).resolves.toBe(false);
    expect(api.legacyReliabilityApprove).toHaveBeenCalledTimes(1);

    finish();
    await expect(first).resolves.toBe(true);
    expect(useLegacyReliabilityStore.getState().approvalPending).toBe(false);
  });

  it("exposes an approval error and clears it for a new proposal", async () => {
    vi.mocked(api.legacyReliabilityApprove).mockRejectedValue(
      new Error("approval expired"),
    );

    await expect(
      useLegacyReliabilityStore
        .getState()
        .approveProposal({ proposalId: 31, attemptId: 47 }),
    ).resolves.toBe(false);
    expect(useLegacyReliabilityStore.getState().approvalError).toContain(
      "approval expired",
    );

    const next = status("observing");
    next.mode = "assisted";
    next.proposal = {
      proposalId: 32,
      attemptId: 48,
      incidentId: 9,
      category: "discord",
      previousConfigId: "discord_1.conf",
      candidateConfigId: "discord_2.conf",
      expiresAtMonotonicMs: 30_000,
    };
    useLegacyReliabilityStore.getState().applyVersionedStatus({
      revision: 1,
      value: next,
    });

    expect(useLegacyReliabilityStore.getState().approvalError).toBe("");
  });
});
