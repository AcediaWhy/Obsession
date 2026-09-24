import { invoke, isTauri } from "@tauri-apps/api/core";

export type OnboardingPhase =
  | "welcome"
  | "readiness"
  | "goals"
  | "recommendation"
  | "review"
  | "applying"
  | "rolling_back"
  | "verifying"
  | "result"
  | "recovery_required";

export type OnboardingPresentation = "required" | "offer" | "modal" | "hidden";
export type OnboardingDestination = "overview" | "dpi" | "ai" | "telegram";

export interface OnboardingGoals {
  dpi: boolean;
  ai: boolean;
  telegram: boolean;
}

export interface OnboardingDraft {
  goals: OnboardingGoals;
  dpiEngine: "legacy" | "zapret2";
  aiProvider: "malw" | "geohide";
}

export interface OnboardingPlanAction {
  kind: "configure_settings" | "install_hosts" | "start_proxy" | "start_dpi";
  title: string;
  detail: string;
  verification: string;
  rollback: string;
}

export interface OnboardingPlan {
  planId: string;
  draft: OnboardingDraft;
  actions: OnboardingPlanAction[];
  dpiConfig: string | null;
  proxyPort: number;
  fakeTlsDomain: string;
}

export type VerificationStatus = "passed" | "failed" | "inconclusive" | "not_selected";
export type VerificationOutcome = "success" | "partial" | "failed" | "inconclusive";

export interface VerificationTarget {
  id: string;
  label: string;
  status: VerificationStatus;
  message: string;
}

export interface VerificationResult {
  outcome: VerificationOutcome;
  targets: VerificationTarget[];
  accepted: boolean;
}

export interface OnboardingTransaction {
  transactionId: string;
  planId: string;
  status: "applying" | "applied" | "rolling_back" | "rolled_back" | "recovery_required";
  checkpoint: string;
  verification: VerificationResult | null;
}

export interface OnboardingSnapshot {
  flowVersion: number;
  revision: number;
  phase: OnboardingPhase;
  presentation: OnboardingPresentation;
  draft: OnboardingDraft;
  plan: OnboardingPlan | null;
  transaction: OnboardingTransaction | null;
  verification: VerificationResult | null;
  terminalStatus: "active" | "completed" | "skipped" | "cancelled";
  destination: OnboardingDestination | null;
}

export interface OnboardingReadiness {
  service: boolean;
  dpi: boolean;
  hosts: boolean;
  telegram: boolean;
  protectedResources: boolean;
  appData: boolean;
  pendingRecovery: boolean;
  proxyPort: boolean;
  repairAvailable: boolean;
}

export interface OnboardingFailure {
  code:
    | "BUSY"
    | "INVALID_DRAFT"
    | "STALE_REVISION"
    | "STALE_PLAN"
    | "RUNTIME_UNAVAILABLE"
    | "PREFLIGHT_FAILED"
    | "APPLY_FAILED"
    | "ROLLBACK_FAILED"
    | "VERIFICATION_REQUIRED"
    | "INVALID_TRANSITION"
    | "REPAIR_UNAVAILABLE";
  retryable: boolean;
  messageCode: string;
  logPath: string | null;
}

export function normalizeOnboardingFailure(value: unknown): OnboardingFailure {
  if (value && typeof value === "object") {
    const failure = value as Partial<OnboardingFailure>;
    if (
      typeof failure.code === "string" &&
      typeof failure.retryable === "boolean" &&
      typeof failure.messageCode === "string" &&
      (failure.logPath === null || typeof failure.logPath === "string")
    ) {
      return failure as OnboardingFailure;
    }
  }
  return {
    code: "PREFLIGHT_FAILED",
    retryable: true,
    messageCode: "onboarding.error.preflight_failed",
    logPath: null,
  };
}

export const onboardingApi = {
  // Старый формат сохраняется для завершения операций предыдущих версий.
  getSnapshot: (): Promise<OnboardingSnapshot | null> => isTauri()
    ? invoke<OnboardingSnapshot | null>("onboarding_get_recovery")
    : Promise.resolve(null),
  verify: (transactionId: string) =>
    invoke<OnboardingSnapshot>("onboarding_verify", { transactionId }),
  acceptVerification: (transactionId: string) =>
    invoke<OnboardingSnapshot>("onboarding_accept_verification", { transactionId }),
  rollback: (transactionId: string) =>
    invoke<OnboardingSnapshot>("onboarding_rollback", { transactionId }),
  complete: (transactionId: string, destination: OnboardingDestination) =>
    invoke<OnboardingSnapshot>("onboarding_complete", { transactionId, destination }),
  launchRepair: () => invoke<void>("launch_repair_setup"),
};
