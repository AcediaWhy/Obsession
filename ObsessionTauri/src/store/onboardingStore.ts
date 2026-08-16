import { create } from "zustand";

import {
  normalizeOnboardingFailure,
  onboardingApi,
  type OnboardingDestination,
  type OnboardingDraft,
  type OnboardingFailure,
  type OnboardingReadiness,
  type OnboardingSnapshot,
} from "../lib/onboarding";

interface OnboardingState {
  loaded: boolean;
  busy: boolean;
  snapshot: OnboardingSnapshot | null;
  readiness: OnboardingReadiness | null;
  failure: OnboardingFailure | null;
  initialize: () => Promise<void>;
  start: (entryPoint: "first_run" | "soft_offer" | "settings") => Promise<boolean>;
  checkReadiness: () => Promise<boolean>;
  saveDraft: (draft: OnboardingDraft) => Promise<boolean>;
  buildPlan: (draft: OnboardingDraft) => Promise<boolean>;
  apply: () => Promise<boolean>;
  verify: () => Promise<boolean>;
  acceptVerification: () => Promise<boolean>;
  rollback: () => Promise<boolean>;
  complete: (destination: OnboardingDestination) => Promise<boolean>;
  skip: () => Promise<boolean>;
  cancel: () => Promise<boolean>;
  launchRepair: () => Promise<boolean>;
  clearFailure: () => void;
}

async function runSnapshotMutation(
  set: (partial: Partial<OnboardingState>) => void,
  request: () => Promise<OnboardingSnapshot>,
  resyncOnFailure = true,
): Promise<boolean> {
  set({ busy: true, failure: null });
  try {
    const snapshot = await request();
    set({ snapshot, busy: false, loaded: true });
    return true;
  } catch (error) {
    const failure = normalizeOnboardingFailure(error);
    if (resyncOnFailure) {
      try {
        const snapshot = await onboardingApi.getSnapshot();
        set({ snapshot, failure, busy: false, loaded: true });
        return false;
      } catch {
        // Keep the last known snapshot if the authoritative state cannot be read.
      }
    }
    set({ failure, busy: false, loaded: true });
    return false;
  }
}

export const useOnboardingStore = create<OnboardingState>((set, get) => ({
  loaded: false,
  busy: false,
  snapshot: null,
  readiness: null,
  failure: null,

  initialize: async () => {
    if (get().busy || get().loaded) return;
    await runSnapshotMutation(set, onboardingApi.getSnapshot, false);
  },

  start: (entryPoint) => runSnapshotMutation(set, () => onboardingApi.start(entryPoint)),

  checkReadiness: async () => {
    set({ busy: true, failure: null });
    try {
      const readiness = await onboardingApi.checkReadiness();
      const snapshot = await onboardingApi.getSnapshot();
      set({ readiness, snapshot, busy: false });
      return true;
    } catch (error) {
      set({ failure: normalizeOnboardingFailure(error), busy: false });
      return false;
    }
  },

  saveDraft: (draft) => {
    const snapshot = get().snapshot;
    return snapshot
      ? runSnapshotMutation(set, () => onboardingApi.saveDraft(draft, snapshot.revision))
      : Promise.resolve(false);
  },

  buildPlan: (draft) => {
    const snapshot = get().snapshot;
    return snapshot
      ? runSnapshotMutation(set, () => onboardingApi.buildPlan(draft, snapshot.revision))
      : Promise.resolve(false);
  },

  apply: () => {
    const plan = get().snapshot?.plan;
    return plan
      ? runSnapshotMutation(set, () => onboardingApi.apply(plan.planId))
      : Promise.resolve(false);
  },

  verify: () => {
    const transaction = get().snapshot?.transaction;
    return transaction
      ? runSnapshotMutation(set, () => onboardingApi.verify(transaction.transactionId))
      : Promise.resolve(false);
  },

  acceptVerification: () => {
    const transaction = get().snapshot?.transaction;
    return transaction
      ? runSnapshotMutation(set, () =>
          onboardingApi.acceptVerification(transaction.transactionId),
        )
      : Promise.resolve(false);
  },

  rollback: () => {
    const transaction = get().snapshot?.transaction;
    return transaction
      ? runSnapshotMutation(set, () => onboardingApi.rollback(transaction.transactionId))
      : Promise.resolve(false);
  },

  complete: (destination) => {
    const transaction = get().snapshot?.transaction;
    return transaction
      ? runSnapshotMutation(set, () =>
          onboardingApi.complete(transaction.transactionId, destination),
        )
      : Promise.resolve(false);
  },

  skip: () => {
    const snapshot = get().snapshot;
    return snapshot
      ? runSnapshotMutation(set, () => onboardingApi.skip(snapshot.revision))
      : Promise.resolve(false);
  },

  cancel: () => {
    const snapshot = get().snapshot;
    return snapshot
      ? runSnapshotMutation(set, () => onboardingApi.cancel(snapshot.revision))
      : Promise.resolve(false);
  },

  launchRepair: async () => {
    set({ busy: true, failure: null });
    try {
      await onboardingApi.launchRepair();
      return true;
    } catch (error) {
      set({ failure: normalizeOnboardingFailure(error), busy: false });
      return false;
    }
  },

  clearFailure: () => set({ failure: null }),
}));
