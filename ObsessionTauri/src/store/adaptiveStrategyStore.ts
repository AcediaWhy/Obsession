import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  api,
  on,
  type AdaptiveCategory,
  type AdaptiveProbeBatch,
  type AdaptiveRecommendationDescriptor,
  type AdaptiveStatus,
  type AdaptiveSuggestion,
  type AdaptiveTransport,
} from "../lib/tauri";
import { toast } from "./toastStore";

interface AdaptiveStrategyState {
  status: AdaptiveStatus | null;
  suggestion: AdaptiveSuggestion | null;
  probe: AdaptiveProbeBatch | null;
  recommendation: AdaptiveRecommendationDescriptor | null;
  recommendationCheckedFor: AdaptiveTransport | null;
  verificationEndsAt: number | null;
  savedCandidateId: string | null;
  busy: boolean;
  error: string;

  bootstrap: () => Promise<UnlistenFn>;
  startSearch: (category: AdaptiveCategory, transport: AdaptiveTransport) => Promise<void>;
  findRecommendation: (
    category: AdaptiveCategory,
    transport: AdaptiveTransport,
  ) => Promise<void>;
  applyRecommendation: (
    category: AdaptiveCategory,
    transport: AdaptiveTransport,
  ) => Promise<void>;
  cancel: () => Promise<void>;
  confirm: () => Promise<void>;
  reject: () => Promise<void>;
  resetSaved: (category: AdaptiveCategory) => Promise<void>;
}

const VERIFICATION_MS = 60_000;

function verificationEnd(status: AdaptiveStatus): number | null {
  return status.phase === "temporary_verification"
    ? Date.now() + VERIFICATION_MS
    : null;
}

export const useAdaptiveStrategyStore = create<AdaptiveStrategyState>((set, get) => ({
  status: null,
  suggestion: null,
  probe: null,
  recommendation: null,
  recommendationCheckedFor: null,
  verificationEndsAt: null,
  savedCandidateId: null,
  busy: false,
  error: "",

  bootstrap: async () => {
    const [initial, unlistenStatus, unlistenSuggestion, unlistenProbe] =
      await Promise.all([
        api.adaptiveGetStatus().catch(() => null),
        on.adaptiveStatus((status) => {
          const previous = get().status;
          set({
            status,
            suggestion: status.phase === "suggested" ? get().suggestion : null,
            verificationEndsAt:
              status.phase === "temporary_verification" &&
              previous?.phase === "temporary_verification"
                ? get().verificationEndsAt
                : verificationEnd(status),
            savedCandidateId:
              status.phase === "applied"
                ? status.candidateId
                : get().savedCandidateId,
          });
        }),
        on.adaptiveSuggestion((suggestion) => set({ suggestion })),
        on.adaptiveProbe((probe) => set({ probe })),
      ]);
    if (initial) {
      set({ status: initial, verificationEndsAt: verificationEnd(initial) });
    }
    return () => {
      unlistenStatus();
      unlistenSuggestion();
      unlistenProbe();
    };
  },

  startSearch: async (category, transport) => {
    set({ busy: true, error: "", probe: null });
    try {
      await api.adaptiveStartSearch(category, transport);
    } catch (error) {
      const message = String(error);
      set({ error: message });
      toast.error(message);
    } finally {
      set({ busy: false });
    }
  },

  findRecommendation: async (category, transport) => {
    set({ busy: true, error: "", recommendation: null });
    try {
      const recommendation = await api.adaptiveGetRecommendation(category, transport);
      set({ recommendation, recommendationCheckedFor: transport });
    } catch (error) {
      const message = String(error);
      set({ error: message });
      toast.error(message);
    } finally {
      set({ busy: false });
    }
  },

  applyRecommendation: async (category, transport) => {
    set({ busy: true, error: "" });
    try {
      const recommendation = await api.adaptiveApplyRecommendation(category, transport);
      set({ recommendation, recommendationCheckedFor: transport });
      toast.success("Рекомендация применена к Gaming + GitHub");
    } catch (error) {
      const message = String(error);
      set({ error: message });
      toast.error(message);
    } finally {
      set({ busy: false });
    }
  },

  cancel: async () => {
    set({ busy: true, error: "" });
    try {
      await api.adaptiveCancelSearch();
    } catch (error) {
      set({ error: String(error) });
    } finally {
      set({ busy: false });
    }
  },

  confirm: async () => {
    const { status } = get();
    if (!status?.sessionId || !status.candidateId) return;
    set({ busy: true, error: "" });
    try {
      await api.adaptiveConfirmCandidate(status.sessionId, status.candidateId);
      toast.success("Стратегия сохранена для этой сети");
    } catch (error) {
      const message = String(error);
      set({ error: message });
      toast.error(message);
    } finally {
      set({ busy: false });
    }
  },

  reject: async () => {
    const { status } = get();
    if (!status?.sessionId || !status.candidateId) return;
    set({ busy: true, error: "" });
    try {
      await api.adaptiveRejectCandidate(status.sessionId, status.candidateId);
    } catch (error) {
      set({ error: String(error) });
    } finally {
      set({ busy: false });
    }
  },

  resetSaved: async (category) => {
    set({ busy: true, error: "" });
    try {
      await api.adaptiveResetSaved(category);
      set({
        savedCandidateId: null,
        recommendation: null,
        recommendationCheckedFor: null,
      });
      toast.success("Сохранённая стратегия сброшена");
    } catch (error) {
      const message = String(error);
      set({ error: message });
      toast.error(message);
    } finally {
      set({ busy: false });
    }
  },
}));
