import { create } from "zustand";

import {
  api,
  type LegacyReliabilityProposal,
  type LegacyReliabilityStatus,
  type VersionedSection,
} from "../lib/tauri";

interface LegacyReliabilityState {
  revision: number;
  status: LegacyReliabilityStatus;
  approvalPending: boolean;
  approvalError: string;
  applyVersionedStatus: (
    section: VersionedSection<LegacyReliabilityStatus>,
  ) => boolean;
  approveProposal: (
    proposal: Pick<LegacyReliabilityProposal, "proposalId" | "attemptId">,
  ) => Promise<boolean>;
  clearApprovalError: () => void;
}

export const INITIAL_LEGACY_RELIABILITY_STATUS: LegacyReliabilityStatus = {
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
};

export const useLegacyReliabilityStore = create<LegacyReliabilityState>(
  (set, get) => ({
    revision: -1,
    status: INITIAL_LEGACY_RELIABILITY_STATUS,
    approvalPending: false,
    approvalError: "",

    applyVersionedStatus: (section) => {
      if (section.revision <= get().revision) return false;
      const currentProposal = get().status.proposal;
      const nextProposal = section.value.proposal;
      const proposalChanged =
        currentProposal?.proposalId !== nextProposal?.proposalId ||
        currentProposal?.attemptId !== nextProposal?.attemptId;
      set({
        revision: section.revision,
        status: section.value,
        ...(proposalChanged ? { approvalError: "" } : {}),
      });
      return true;
    },

    approveProposal: async (proposal) => {
      if (get().approvalPending) return false;
      set({ approvalPending: true, approvalError: "" });
      try {
        await api.legacyReliabilityApprove(
          proposal.proposalId,
          proposal.attemptId,
        );
        return true;
      } catch (error) {
        set({ approvalError: String(error) });
        return false;
      } finally {
        set({ approvalPending: false });
      }
    },

    clearApprovalError: () => set({ approvalError: "" }),
  }),
);
