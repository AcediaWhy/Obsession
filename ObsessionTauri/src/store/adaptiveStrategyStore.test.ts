import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("../lib/tauri", () => ({
  api: {},
}));

import { useAdaptiveStrategyStore } from "./adaptiveStrategyStore";
import type { AdaptiveStatus } from "../lib/tauri";

function status(partial: Partial<AdaptiveStatus>): AdaptiveStatus {
  return {
    phase: "idle",
    category: null,
    diagnosis: null,
    sessionId: null,
    attemptId: null,
    candidateId: null,
    candidateIndex: null,
    candidateTotal: null,
    verificationDeadlineMs: null,
    rollbackReason: null,
    transport: null,
    sessionMode: null,
    currentRound: null,
    totalRounds: null,
    failureStage: null,
    ...partial,
  };
}

function reset() {
  useAdaptiveStrategyStore.setState({
    revision: -1,
    status: null,
    suggestion: null,
    verificationEndsAt: null,
    savedCandidateId: null,
  });
}

describe("adaptiveStrategyStore.applyStatus", () => {
  beforeEach(reset);

  it("ставит verificationEndsAt при входе в temporary_verification", () => {
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "temporary_verification" }));
    const ends = useAdaptiveStrategyStore.getState().verificationEndsAt;
    expect(ends).not.toBeNull();
    expect(ends!).toBeGreaterThan(Date.now());
  });

  it("сохраняет прежний verificationEndsAt при verification→verification (countdown не мёрзнет и не прыгает)", () => {
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "temporary_verification" }));
    const first = useAdaptiveStrategyStore.getState().verificationEndsAt;
    // Повторный статус той же фазы (напр. resume из трея) не сбрасывает дедлайн.
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "temporary_verification" }));
    expect(useAdaptiveStrategyStore.getState().verificationEndsAt).toBe(first);
  });

  it("сбрасывает verificationEndsAt при выходе из verification", () => {
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "temporary_verification" }));
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "searching" }));
    expect(useAdaptiveStrategyStore.getState().verificationEndsAt).toBeNull();
  });

  it("запоминает savedCandidateId на фазе applied", () => {
    useAdaptiveStrategyStore
      .getState()
      .applyStatus(status({ phase: "applied", candidateId: "cand-7" }));
    expect(useAdaptiveStrategyStore.getState().savedCandidateId).toBe("cand-7");
  });

  it("гасит suggestion на любой фазе кроме suggested", () => {
    useAdaptiveStrategyStore.setState({
      suggestion: { category: "discord", reason: "repeated_reset" },
    });
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "searching" }));
    expect(useAdaptiveStrategyStore.getState().suggestion).toBeNull();
  });

  it("сохраняет suggestion, пока фаза suggested", () => {
    const suggestion = { category: "discord" as const, reason: "repeated_reset" as const };
    useAdaptiveStrategyStore.setState({ suggestion });
    useAdaptiveStrategyStore.getState().applyStatus(status({ phase: "suggested" }));
    expect(useAdaptiveStrategyStore.getState().suggestion).toEqual(suggestion);
  });

  it("применяет null и отбрасывает stale adaptive section", () => {
    expect(
      useAdaptiveStrategyStore.getState().applyVersionedStatus({
        revision: 2,
        value: status({ phase: "searching" }),
      }),
    ).toBe(true);
    expect(
      useAdaptiveStrategyStore.getState().applyVersionedStatus({
        revision: 1,
        value: status({ phase: "idle" }),
      }),
    ).toBe(false);
    expect(useAdaptiveStrategyStore.getState().status?.phase).toBe("searching");

    expect(
      useAdaptiveStrategyStore.getState().applyVersionedStatus({
        revision: 3,
        value: null,
      }),
    ).toBe(true);
    expect(useAdaptiveStrategyStore.getState().status).toBeNull();
    expect(useAdaptiveStrategyStore.getState().revision).toBe(3);
  });
});
