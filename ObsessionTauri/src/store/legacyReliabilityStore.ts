import { create } from "zustand";

import type {
  LegacyReliabilityStatus,
  VersionedSection,
} from "../lib/tauri";

interface LegacyReliabilityState {
  revision: number;
  status: LegacyReliabilityStatus;
  applyVersionedStatus: (
    section: VersionedSection<LegacyReliabilityStatus>,
  ) => boolean;
}

const INITIAL_STATUS: LegacyReliabilityStatus = {
  mode: "observe_only",
  phase: "inactive",
  activeCategories: [],
  sessionId: null,
  sensorGeneration: null,
};

export const useLegacyReliabilityStore = create<LegacyReliabilityState>(
  (set, get) => ({
    revision: -1,
    status: INITIAL_STATUS,

    applyVersionedStatus: (section) => {
      if (section.revision <= get().revision) return false;
      set({ revision: section.revision, status: section.value });
      return true;
    },
  }),
);
