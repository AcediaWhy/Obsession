import { create } from "zustand";

import type {
  BrainStatus,
  VersionedSection,
} from "../lib/tauri";

interface BrainState {
  revision: number;
  status: BrainStatus | null;
  applyVersionedStatus: (
    section: VersionedSection<BrainStatus | null>,
  ) => boolean;
  clearLocalStatus: () => void;
}

export const useBrainStore = create<BrainState>((set, get) => ({
  revision: -1,
  status: null,

  applyVersionedStatus: (section) => {
    if (section.revision <= get().revision) return false;
    set({ revision: section.revision, status: section.value });
    return true;
  },

  clearLocalStatus: () => set({ status: null }),
}));
