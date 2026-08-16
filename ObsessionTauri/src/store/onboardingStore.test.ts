import { beforeEach, describe, expect, it, vi } from "vitest";

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    start: vi.fn(),
    getSnapshot: vi.fn(),
    checkReadiness: vi.fn(),
    saveDraft: vi.fn(),
    buildPlan: vi.fn(),
    apply: vi.fn(),
    getTransaction: vi.fn(),
    verify: vi.fn(),
    acceptVerification: vi.fn(),
    rollback: vi.fn(),
    complete: vi.fn(),
    skip: vi.fn(),
    cancel: vi.fn(),
    launchRepair: vi.fn(),
  },
}));

vi.mock("../lib/onboarding", () => ({
  onboardingApi: apiMock,
  normalizeOnboardingFailure: (value: unknown) =>
    value && typeof value === "object"
      ? value
      : {
          code: "PREFLIGHT_FAILED",
          retryable: true,
          messageCode: "onboarding.error.preflight_failed",
          logPath: null,
        },
}));

import type { OnboardingSnapshot } from "../lib/onboarding";
import { useOnboardingStore } from "./onboardingStore";

const snapshot: OnboardingSnapshot = {
  flowVersion: 2,
  revision: 7,
  phase: "welcome",
  presentation: "offer",
  draft: {
    goals: { dpi: true, ai: false, telegram: false },
    dpiEngine: "legacy",
    aiProvider: "malw",
  },
  plan: null,
  transaction: null,
  verification: null,
  terminalStatus: "completed",
  destination: null,
};

describe("onboarding store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useOnboardingStore.setState({
      loaded: false,
      busy: false,
      snapshot: null,
      readiness: null,
      failure: null,
    });
  });

  it("hydrates an existing user as a non-blocking offer", async () => {
    apiMock.getSnapshot.mockResolvedValue(snapshot);
    await useOnboardingStore.getState().initialize();
    expect(useOnboardingStore.getState().snapshot?.presentation).toBe("offer");
    expect(useOnboardingStore.getState().loaded).toBe(true);
  });

  it("passes the authoritative revision with a multi-goal draft", async () => {
    useOnboardingStore.setState({ loaded: true, snapshot });
    const draft = {
      ...snapshot.draft,
      goals: { dpi: true, ai: true, telegram: true },
    };
    apiMock.saveDraft.mockResolvedValue({
      ...snapshot,
      revision: 8,
      phase: "recommendation",
      presentation: "modal",
      draft,
    });
    expect(await useOnboardingStore.getState().saveDraft(draft)).toBe(true);
    expect(apiMock.saveDraft).toHaveBeenCalledWith(draft, 7);
    expect(useOnboardingStore.getState().snapshot?.phase).toBe("recommendation");
  });

  it("turns an unstructured rejection into a safe typed failure", async () => {
    apiMock.getSnapshot.mockRejectedValue("raw Windows error <script>");
    await useOnboardingStore.getState().initialize();
    expect(useOnboardingStore.getState().failure).toEqual({
      code: "PREFLIGHT_FAILED",
      retryable: true,
      messageCode: "onboarding.error.preflight_failed",
      logPath: null,
    });
  });

  it("resyncs the authoritative snapshot after a failed mutation rolls back", async () => {
    const review: OnboardingSnapshot = {
      ...snapshot,
      phase: "review",
      presentation: "modal",
      terminalStatus: "active",
      plan: {
        planId: "plan-test",
        draft: snapshot.draft,
        actions: [],
        dpiConfig: "discord-test.cmd",
        proxyPort: 1443,
        fakeTlsDomain: "",
      },
    };
    const rolledBack: OnboardingSnapshot = {
      ...review,
      revision: 8,
      phase: "result",
      transaction: {
        transactionId: "tx-test",
        planId: "plan-test",
        status: "rolled_back",
        checkpoint: "rollback_complete",
        verification: null,
      },
    };
    useOnboardingStore.setState({ loaded: true, snapshot: review });
    apiMock.apply.mockRejectedValue({
      code: "APPLY_FAILED",
      retryable: true,
      messageCode: "onboarding.error.apply_failed",
      logPath: "C:\\Users\\User\\AppData\\Roaming\\Obsession\\onboarding.log",
    });
    apiMock.getSnapshot.mockResolvedValue(rolledBack);

    expect(await useOnboardingStore.getState().apply()).toBe(false);
    expect(apiMock.apply).toHaveBeenCalledWith("plan-test");
    expect(useOnboardingStore.getState().snapshot?.phase).toBe("result");
    expect(useOnboardingStore.getState().snapshot?.transaction?.status).toBe("rolled_back");
    expect(useOnboardingStore.getState().failure?.code).toBe("APPLY_FAILED");
  });
});
