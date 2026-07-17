import { create } from "zustand";

import {
  api,
  type AdaptiveCategory,
  type AdaptiveProbeBatch,
  type AdaptiveRecommendationDescriptor,
  type AdaptiveStatus,
  type AdaptiveSuggestion,
  type AdaptiveTransport,
  type VersionedSection,
} from "../lib/tauri";
import { toast } from "./toastStore";

interface AdaptiveStrategyState {
  revision: number;
  status: AdaptiveStatus | null;
  suggestion: AdaptiveSuggestion | null;
  probe: AdaptiveProbeBatch | null;
  recommendation: AdaptiveRecommendationDescriptor | null;
  recommendationCheckedFor: AdaptiveTransport | null;
  verificationEndsAt: number | null;
  savedCandidateId: string | null;
  busy: boolean;
  error: string;

  applyVersionedStatus: (
    section: VersionedSection<AdaptiveStatus | null>,
  ) => boolean;
  applyStatus: (status: AdaptiveStatus) => void;
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

function statusPatch(
  status: AdaptiveStatus,
  previous: AdaptiveStatus | null,
  suggestion: AdaptiveSuggestion | null,
  verificationEndsAt: number | null,
  savedCandidateId: string | null,
) {
  return {
    status,
    suggestion: status.phase === "suggested" ? suggestion : null,
    verificationEndsAt:
      status.phase === "temporary_verification" &&
      previous?.phase === "temporary_verification"
        ? verificationEndsAt
        : verificationEnd(status),
    savedCandidateId:
      status.phase === "applied" ? status.candidateId : savedCandidateId,
  };
}

export const useAdaptiveStrategyStore = create<AdaptiveStrategyState>((set, get) => ({
  revision: -1,
  status: null,
  suggestion: null,
  probe: null,
  recommendation: null,
  recommendationCheckedFor: null,
  verificationEndsAt: null,
  savedCandidateId: null,
  busy: false,
  error: "",

  // Применяет adaptive://status (из подписки И из runtime-снапшота при возврате
  // из трея). Единый путь: обе точки входа сохраняют verificationEndsAt /
  // savedCandidateId / suggestion, иначе resume-снапшот замораживал бы countdown
  // верификации и терял сохранённый candidateId.
  applyVersionedStatus: (section) => {
    if (section.revision <= get().revision) return false;
    if (!section.value) {
      set({
        revision: section.revision,
        status: null,
        suggestion: null,
        verificationEndsAt: null,
      });
      return true;
    }
    const state = get();
    set({
      revision: section.revision,
      ...statusPatch(
        section.value,
        state.status,
        state.suggestion,
        state.verificationEndsAt,
        state.savedCandidateId,
      ),
    });
    return true;
  },

  applyStatus: (status) => {
    const state = get();
    set(
      statusPatch(
        status,
        state.status,
        state.suggestion,
        state.verificationEndsAt,
        state.savedCandidateId,
      ),
    );
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
