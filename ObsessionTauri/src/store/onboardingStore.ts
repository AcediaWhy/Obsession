import { create } from "zustand";

import {
  normalizeOnboardingFailure,
  onboardingApi,
  type OnboardingDestination,
  type OnboardingFailure,
  type OnboardingSnapshot,
} from "../lib/onboarding";

interface OnboardingState {
  loaded: boolean;
  busy: boolean;
  snapshot: OnboardingSnapshot | null;
  failure: OnboardingFailure | null;
  initialize: () => Promise<void>;
  refresh: () => Promise<boolean>;
  verify: () => Promise<boolean>;
  rollback: () => Promise<boolean>;
  complete: (destination: OnboardingDestination) => Promise<boolean>;
  launchRepair: () => Promise<boolean>;
}

async function runSnapshotMutation(
  set: (partial: Partial<OnboardingState>) => void,
  request: () => Promise<OnboardingSnapshot | null>,
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
        // Если журнал недоступен, сохраняем последний известный снимок.
      }
    }
    set({ failure, busy: false, loaded: true });
    return false;
  }
}

export const useOnboardingStore = create<OnboardingState>((set, get) => {
  const run = (request: () => Promise<OnboardingSnapshot | null>, resync = true) =>
    get().busy ? Promise.resolve(false) : runSnapshotMutation(set, request, resync);

  return {
    loaded: false,
    busy: false,
    snapshot: null,
    failure: null,

    initialize: async () => {
      if (get().loaded) return;
      await run(onboardingApi.getSnapshot, false);
    },
    refresh: () => run(onboardingApi.getSnapshot, false),

    verify: () => {
      const transaction = get().snapshot?.transaction;
      return transaction?.status === "applied"
        ? run(() => onboardingApi.verify(transaction.transactionId))
        : Promise.resolve(false);
    },

    rollback: () => {
      const transaction = get().snapshot?.transaction;
      return transaction && transaction.status !== "rolled_back"
        ? run(() => onboardingApi.rollback(transaction.transactionId))
        : Promise.resolve(false);
    },

    complete: (destination) => {
      const transaction = get().snapshot?.transaction;
      if (!transaction) return Promise.resolve(false);
      const rolledBack = transaction.status === "rolled_back";
      if (!rolledBack && (transaction.status !== "applied" || !transaction.verification)) {
        return Promise.resolve(false);
      }
      return run(async () => {
        // Принятие частичного результата требует явного нажатия кнопки сохранения.
        if (!rolledBack && transaction.verification?.outcome !== "success" && !transaction.verification?.accepted) {
          await onboardingApi.acceptVerification(transaction.transactionId);
        }
        return onboardingApi.complete(transaction.transactionId, destination);
      });
    },

    launchRepair: async () => {
      if (get().busy) return false;
      set({ busy: true, failure: null });
      try {
        await onboardingApi.launchRepair();
        return true;
      } catch (error) {
        set({ failure: normalizeOnboardingFailure(error) });
        return false;
      } finally {
        set({ busy: false });
      }
    },
  };
});
