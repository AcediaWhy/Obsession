import { beforeEach, describe, expect, it, vi } from "vitest";

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    getSnapshot: vi.fn(), verify: vi.fn(), acceptVerification: vi.fn(),
    rollback: vi.fn(), complete: vi.fn(), launchRepair: vi.fn(),
  },
}));
vi.mock("../lib/onboarding", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/onboarding")>(),
  onboardingApi: apiMock,
}));

import type { OnboardingSnapshot } from "../lib/onboarding";
import { useOnboardingStore } from "./onboardingStore";

function pending(): OnboardingSnapshot {
  return {
    flowVersion: 2, revision: 7, phase: "recovery_required", presentation: "required",
    draft: { goals: { dpi: true, ai: false, telegram: false }, dpiEngine: "legacy", aiProvider: "malw" },
    plan: null, verification: null, terminalStatus: "active", destination: null,
    transaction: { transactionId: "tx-test", planId: "plan-test", status: "recovery_required", checkpoint: "proxy_started", verification: null },
  };
}

describe("legacy setup recovery store", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useOnboardingStore.setState({ loaded: false, busy: false, snapshot: null, failure: null });
  });

  it("opens a fresh installation without starting or applying a setup", async () => {
    apiMock.getSnapshot.mockResolvedValue(null);
    await useOnboardingStore.getState().initialize();
    expect(useOnboardingStore.getState()).toMatchObject({ loaded: true, snapshot: null, failure: null });
    expect(apiMock.verify).not.toHaveBeenCalled();
    expect(apiMock.rollback).not.toHaveBeenCalled();
    expect(apiMock.complete).not.toHaveBeenCalled();
  });

  it("loads an interrupted transaction without automatically rolling it back", async () => {
    apiMock.getSnapshot.mockResolvedValue(pending());
    await useOnboardingStore.getState().initialize();
    expect(useOnboardingStore.getState().snapshot?.transaction?.transactionId).toBe("tx-test");
    expect(apiMock.rollback).not.toHaveBeenCalled();
  });

  it("does not issue duplicate startup reads while loading", async () => {
    let resolve!: (value: null) => void;
    apiMock.getSnapshot.mockReturnValue(new Promise<null>((done) => { resolve = done; }));
    const first = useOnboardingStore.getState().initialize();
    await useOnboardingStore.getState().initialize();
    expect(apiMock.getSnapshot).toHaveBeenCalledTimes(1);
    resolve(null);
    await first;
  });

  it("keeps a read failure visible and allows retry", async () => {
    apiMock.getSnapshot.mockRejectedValueOnce("raw filesystem error");
    await useOnboardingStore.getState().initialize();
    expect(useOnboardingStore.getState().failure?.code).toBe("PREFLIGHT_FAILED");
    apiMock.getSnapshot.mockResolvedValue(pending());
    expect(await useOnboardingStore.getState().refresh()).toBe(true);
    expect(useOnboardingStore.getState().failure).toBeNull();
  });

  it("retains the journal after a failed rollback and resyncs the checkpoint", async () => {
    useOnboardingStore.setState({ loaded: true, snapshot: pending() });
    const failed = pending();
    failed.transaction!.checkpoint = "hosts_rolled_back";
    apiMock.rollback.mockRejectedValue({ code: "ROLLBACK_FAILED", retryable: true, messageCode: "onboarding.error.rollback_deferred", logPath: null });
    apiMock.getSnapshot.mockResolvedValue(failed);
    expect(await useOnboardingStore.getState().rollback()).toBe(false);
    expect(apiMock.rollback).toHaveBeenCalledWith("tx-test");
    expect(useOnboardingStore.getState().snapshot?.transaction?.checkpoint).toBe("hosts_rolled_back");
    expect(useOnboardingStore.getState().failure?.code).toBe("ROLLBACK_FAILED");
  });

  it("does not complete an interrupted or unverified transaction", async () => {
    for (const status of ["recovery_required", "applied"] as const) {
      const snapshot = pending();
      snapshot.transaction!.status = status;
      useOnboardingStore.setState({ loaded: true, snapshot });
      expect(await useOnboardingStore.getState().complete("overview")).toBe(false);
    }
    expect(apiMock.complete).not.toHaveBeenCalled();
  });

  it("accepts partial results only on explicit completion", async () => {
    const snapshot = pending();
    snapshot.transaction!.status = "applied";
    snapshot.transaction!.verification = { outcome: "partial", targets: [], accepted: false };
    apiMock.getSnapshot.mockResolvedValue(snapshot);
    await useOnboardingStore.getState().initialize();
    expect(apiMock.acceptVerification).not.toHaveBeenCalled();
    apiMock.acceptVerification.mockResolvedValue(snapshot);
    apiMock.complete.mockResolvedValue({ ...snapshot, terminalStatus: "completed" });
    expect(await useOnboardingStore.getState().complete("overview")).toBe(true);
    expect(apiMock.acceptVerification).toHaveBeenCalledWith("tx-test");
    expect(apiMock.complete).toHaveBeenCalledWith("tx-test", "overview");
    expect(useOnboardingStore.getState().snapshot?.terminalStatus).toBe("completed");
  });

  it("does not complete when accepting a partial result fails", async () => {
    const snapshot = pending();
    snapshot.transaction!.status = "applied";
    snapshot.transaction!.verification = { outcome: "partial", targets: [], accepted: false };
    useOnboardingStore.setState({ snapshot });
    apiMock.acceptVerification.mockRejectedValue("disk full");
    apiMock.getSnapshot.mockResolvedValue(snapshot);
    expect(await useOnboardingStore.getState().complete("overview")).toBe(false);
    expect(apiMock.complete).not.toHaveBeenCalled();
  });

  it("closes a completed rollback without requiring a network check", async () => {
    const snapshot = pending();
    snapshot.transaction!.status = "rolled_back";
    useOnboardingStore.setState({ snapshot });
    apiMock.complete.mockResolvedValue({ ...snapshot, terminalStatus: "completed" });
    expect(await useOnboardingStore.getState().complete("overview")).toBe(true);
    expect(apiMock.acceptVerification).not.toHaveBeenCalled();
    expect(apiMock.verify).not.toHaveBeenCalled();
  });

  it("releases the busy state after launching repair", async () => {
    apiMock.launchRepair.mockResolvedValue(undefined);
    expect(await useOnboardingStore.getState().launchRepair()).toBe(true);
    expect(useOnboardingStore.getState().busy).toBe(false);
  });
});
