import { create } from "zustand";
import {
  api, runtime, type Zapret2CatalogProfile, type Zapret2PickMode,
  type Zapret2PickerProgress, type Zapret2PickerResult,
  type Zapret2Scenario, type Zapret2ScenarioProgress, type Zapret2ScenarioResult, type Zapret2Observation,
  type Zapret2HistoryRecord,
} from "../lib/tauri";
import { useDpiStore } from "./dpiStore";
import { useSettingsStore } from "./settingsStore";

export const zapret2ChoiceKey = (category: string, transport: string) => `${category}:${transport}`;
export const zapret2ScenarioKey = (runId: string, profileId: string, scenario: Zapret2Scenario) => `${runId}:${profileId}:${scenario}`;

export function scenarioBlocksApplication(scenarios: Record<string, Zapret2ScenarioResult>, result: Zapret2PickerResult,
  profileId: string): boolean {
  return Object.values(scenarios).some((entry) => entry.runId === result.tournament?.runId && entry.profileId === profileId
    && entry.scenario !== "discord_voice" && !entry.recommendationAvailable);
}

export function applicablePickerResult(result: Zapret2PickerResult | undefined): boolean {
  return !!result && result.status === "passed" && !!result.profileId
    && result.report?.status === "passed" && result.report.passed
    && result.report.checks.length > 0 && result.report.checks.every((check) => check.passed)
    && confirmedPickerCandidate(result, result.profileId);
}

export function confirmedPickerCandidate(result: Zapret2PickerResult | undefined, profileId: string | undefined): boolean {
  if (!result?.tournament || !profileId || !["passed", "tie"].includes(result.status)
    || !["completed", "budget"].includes(result.tournament.stopReason)) return false;
  const candidate = result.tournament.candidates.find((entry) => entry.profileId === profileId);
  if (result.transport === "tls") {
    const rounds = result.tournament.rounds.filter((round) => round.profileId === profileId);
    const allowedProfiles = candidate?.composition
      ? [candidate.composition.apiProfileId, candidate.composition.cdnProfileId] : [profileId];
    if (!candidate || candidate.routingVerifiedRounds !== candidate.rounds || rounds.length !== candidate.rounds
      || !rounds.every((round) => round.testerVersion === "zapret2-functions-v5-routing"
        && round.execution === "flow_routing_verified" && round.runtimeGeneration != null
        && round.runId === result.tournament!.runId && round.transport === result.transport
        && round.networkBefore != null && round.networkBefore === round.networkAfter
        && round.comparable && round.fingerprint === candidate.fingerprint
        && round.functions.some((entry) => entry.required)
        && round.functions.filter((entry) => entry.required).every((entry) => entry.connections?.length
          && entry.connections.every((connection) => connection.status === "verified"
            && allowedProfiles.includes(connection.expectedProfileId)
            && connection.observedSends?.some((action) => connection.observedActions.includes(action))
            && connection.observedProfiles.length === 1 && connection.observedProfiles[0] === connection.expectedProfileId)))) return false;
    if (new Set(rounds.map((round) => round.runtimeGeneration)).size !== rounds.length) return false;
  }
  return !!candidate && candidate.status === "confirmed" && candidate.completeRounds === candidate.rounds
    && candidate.completeRounds >= Math.max(2, result.tournament.requiredConfirmations)
    && candidate.functions.some((entry) => entry.required)
    && candidate.functions.filter((entry) => entry.required).every((entry) => entry.passed === candidate.rounds
      && entry.failed === 0 && entry.inconclusive === 0 && entry.cancelled === 0 && entry.notTested === 0);
}

interface Zapret2StrategyState {
  catalog: Zapret2CatalogProfile[];
  loading: boolean;
  busy: boolean;
  operation: "pick" | "select" | "scenario" | null;
  cancelling: boolean;
  progress: Zapret2PickerProgress | null;
  results: Record<string, Zapret2PickerResult>;
  scenarioProgress: Zapret2ScenarioProgress | null;
  scenarios: Record<string, Zapret2ScenarioResult>;
  observing: boolean;
  history: Zapret2HistoryRecord[];
  historyError: string;
  loadHistory: () => Promise<void>;
  testScenario: (category: string, transport: string, profileId: string, scenario: Zapret2Scenario) => Promise<void>;
  observeScenario: (observation: Zapret2Observation) => Promise<void>;
  applyScenarioProgress: (progress: Zapret2ScenarioProgress) => void;
  error: string;
  load: (categories: string[]) => Promise<void>;
  select: (category: string, transport: string, profileId: string) => Promise<void>;
  pick: (category: string, transport: string, mode: Zapret2PickMode, composition?: {apiProfileId:string;cdnProfileId:string}) => Promise<void>;
  applyResult: (category: string, transport: string, profileId?: string) => Promise<void>;
  cancel: () => Promise<void>;
  applyProgress: (progress: Zapret2PickerProgress) => void;
}

let catalogRequest = 0;

async function refreshRuntime() {
  const snapshot = await runtime.bootstrap();
  useDpiStore.getState().applyVersionedStatus(snapshot.dpi);
  useSettingsStore.getState().applyBootstrap(snapshot.settings);
}

function blocked() {
  const dpi = useDpiStore.getState();
  return dpi.transitioning || dpi.testing || !useSettingsStore.getState().protectedDpiAvailable;
}

export const useZapret2StrategyStore = create<Zapret2StrategyState>((set, get) => ({
  catalog: [], loading: false, busy: false, operation: null, cancelling: false,
  progress: null, results: {}, error: "", scenarioProgress: null, scenarios: {}, observing: false,
  history: [], historyError: "",
  loadHistory: async () => {
    try { set({ history: await api.dpiZapret2History(), historyError: "" }); }
    catch (error) { set({ historyError: String(error) }); }
  },
  load: async (categories) => {
    const request = ++catalogRequest;
    set({ loading: true });
    try {
      const catalog = await api.dpiZapret2Catalog(categories);
      if (request === catalogRequest) set({ catalog, loading: false });
    } catch (error) {
      if (request === catalogRequest) set({ catalog: [], loading: false, error: String(error) });
    }
  },
  select: async (category, transport, profileId) => {
    if (get().busy || blocked() || !useDpiStore.getState().selectedCategories.includes(category)) return;
    const profile = get().catalog.find((item) => item.category === category && item.transport === transport && item.profileId === profileId);
    if (!profile || (profile.dataPlane && transport !== "voice")) return;
    set({ busy: true, operation: "select", error: "" });
    useDpiStore.setState({ transitioning: true });
    try {
      await api.dpiZapret2Select(category, transport, profileId);
      set({ results: {} });
    } catch (error) {
      set({ error: String(error) });
    } finally {
      try { await refreshRuntime(); }
      catch (error) { set({ error: String(error) }); }
      await Promise.all([get().load(useDpiStore.getState().selectedCategories), useDpiStore.getState().loadZapret2Profiles()]);
      useDpiStore.setState({ transitioning: false });
      set({ busy: false, operation: null });
    }
  },
  pick: async (category, transport, mode, composition) => {
    if (get().busy || blocked() || !useDpiStore.getState().selectedCategories.includes(category)
      || (transport !== "tls" && !(category === "youtube_twitch" && transport === "quic"))) return;
    if (composition && (category !== "discord" || transport !== "tls" || composition.apiProfileId === composition.cdnProfileId
      || [composition.apiProfileId,composition.cdnProfileId].some((id) => !get().catalog.some((profile) =>
        profile.category === "discord" && profile.transport === "tls" && profile.profileId === id && profile.compositionEligible)))) return;
    const key = zapret2ChoiceKey(category, transport);
    set((state) => {
      const results = { ...state.results };
      delete results[key];
      return { busy: true, operation: "pick", cancelling: false, error: "", results,
        progress: { category, transport, current: 0, total: 0, profileId: "", label: "", phase: composition ? "composition" : "testing" } };
    });
    useDpiStore.setState({ testing: true, testCancel: false, testingLabel: category });
    try {
      const result = composition ? await api.dpiZapret2Pick(category, transport, mode, composition) : await api.dpiZapret2Pick(category, transport, mode);
      if (result.category !== category || result.transport !== transport) throw new Error("Ответ подбора относится к другому сервису.");
      // Cancellation cannot become an applicable result if the backend response races it.
      const retained = get().cancelling ? { ...result, status: "cancelled" as const, profileId: undefined } : result;
      if (retained.status === "passed" && !applicablePickerResult(retained)) {
        set({ error: "Все проверки не подтверждены. Исходная конфигурация сохранена." });
      }
      set((state) => ({ results: { ...state.results, [key]: retained } }));
    } catch (error) {
      set({ error: String(error) });
    } finally {
      try { await refreshRuntime(); }
      catch (error) { set({ error: String(error) }); }
      await Promise.all([get().load(useDpiStore.getState().selectedCategories), useDpiStore.getState().loadZapret2Profiles()]);
      await get().loadHistory();
      useDpiStore.setState({ testing: false, testCancel: false, testingLabel: "" });
      set({ busy: false, operation: null, cancelling: false, progress: null });
    }
  },
  applyResult: async (category, transport, profileId) => {
    const result = get().results[zapret2ChoiceKey(category, transport)];
    const chosen = profileId ?? result?.profileId;
    if (get().busy || blocked() || !useDpiStore.getState().selectedCategories.includes(category)
      || !confirmedPickerCandidate(result, chosen) || !result.tournament
      || scenarioBlocksApplication(get().scenarios, result, chosen!)) return;
    set({ busy: true, operation: "select", error: "" });
    useDpiStore.setState({ transitioning: true });
    try {
      await api.dpiZapret2ApplyResult(result.tournament.runId, category, transport, chosen!);
      // Any runtime change invalidates the context of recommendations for all
      // slots, even if their own profile IDs did not change.
      set({ results: {} });
    } catch (error) { set({ error: String(error) }); }
    finally {
      try { await refreshRuntime(); } catch (error) { set({ error: String(error) }); }
      await Promise.all([get().load(useDpiStore.getState().selectedCategories), useDpiStore.getState().loadZapret2Profiles()]);
      useDpiStore.setState({ transitioning: false });
      set({ busy: false, operation: null });
    }
  },
  testScenario: async (category, transport, profileId, scenario) => {
    const key = zapret2ChoiceKey(category, transport);
    const result = get().results[key];
    const allowed = scenario === "youtube_video" ? category === "youtube_twitch" && ["tls", "quic"].includes(transport)
      : category === "discord" && transport === "tls";
    if (get().busy || blocked() || !allowed || !useDpiStore.getState().selectedCategories.includes(category)
      || !confirmedPickerCandidate(result, profileId) || !result.tournament) return;
    const runId = result.tournament.runId;
    const requestId = crypto.randomUUID();
    set({ busy: true, operation: "scenario", cancelling: false, observing: false, error: "",
      scenarioProgress: { requestId, trialId: "", runId, category, transport, profileId, scenario, phase: "starting",
        stepId: null, stepIndex: 0, total: 0, label: "Запускаем испытание", instruction: "Дождись первого шага.",
        confirmAfterUnixMs: 0, expiresUnixMs: 0 } });
    useDpiStore.setState({ testing: true, testCancel: false, testingLabel: "Проверка в приложении" });
    try {
      const measured = await api.dpiZapret2TestScenario(requestId, runId, category, transport, profileId, scenario);
      if (measured.requestId !== requestId || measured.runId !== runId || measured.category !== category || measured.transport !== transport
        || measured.profileId !== profileId || measured.scenario !== scenario || measured.source !== "user_report"
        || measured.scope !== "whole_configuration") throw new Error("Ответ испытания относится к другому сценарию.");
      const retained = get().cancelling ? { ...measured, status: "cancelled" as const } : measured;
      set((state) => ({ scenarios: { ...state.scenarios, [zapret2ScenarioKey(runId, profileId, scenario)]: retained },
        results: measured.contextValid ? { [key]: result } : {} }));
      if (measured.failure) set({ error: measured.failure });
    } catch (error) { set({ error: String(error), results: {} }); }
    finally {
      try { await refreshRuntime(); } catch (error) { set({ error: String(error) }); }
      await Promise.all([get().load(useDpiStore.getState().selectedCategories), useDpiStore.getState().loadZapret2Profiles()]);
      await get().loadHistory();
      useDpiStore.setState({ testing: false, testCancel: false, testingLabel: "" });
      set({ busy: false, operation: null, cancelling: false, observing: false, scenarioProgress: null });
    }
  },
  observeScenario: async (observation) => {
    const progress = get().scenarioProgress;
    if (get().operation !== "scenario" || get().cancelling || get().observing || progress?.phase !== "step"
      || !progress.trialId || !progress.stepId || (observation === "passed" && Date.now() < progress.confirmAfterUnixMs)) return;
    set({ observing: true, error: "" });
    try {
      await api.dpiZapret2ScenarioObserve(progress.trialId, progress.stepId, observation);
      const current = get().scenarioProgress;
      if (current?.trialId === progress.trialId && current.stepId === progress.stepId) {
        set({ scenarioProgress: { ...current, phase: "waiting", stepId: null, instruction: "Готовим следующий шаг…" } });
      }
    } catch (error) { set({ error: String(error) }); }
    finally { set({ observing: false }); }
  },
  applyScenarioProgress: (progress) => {
    const current = get().scenarioProgress;
    if (get().operation !== "scenario" || !current || progress.requestId !== current.requestId || progress.runId !== current.runId
      || progress.category !== current.category || progress.transport !== current.transport
      || progress.profileId !== current.profileId || progress.scenario !== current.scenario
      || (current.trialId && progress.trialId !== current.trialId) || progress.phase === "idle") return;
    if ((current.phase === "restoring" && progress.phase !== "restoring")
      || (progress.phase === "step" && progress.stepIndex < current.stepIndex)
      || (progress.phase === "starting" && current.phase !== "starting")) return;
    set({ scenarioProgress: progress });
    // An early cancel may precede the backend's initialization of its cancel
    // flag. Reassert it once when this exact request is acknowledged as active.
    if (get().cancelling && !current.trialId && progress.trialId) {
      void api.dpiTestCancel().catch((error) => {
        if (get().operation === "scenario" && get().scenarioProgress?.requestId === progress.requestId) {
          set({ error: String(error) });
        }
      });
    }
  },
  cancel: async () => {
    if (!["pick", "scenario"].includes(get().operation ?? "") || get().cancelling) return;
    set({ cancelling: true });
    useDpiStore.setState({ testCancel: true, testingLabel: "Возвращаем исходную конфигурацию…" });
    try { await api.dpiTestCancel(); }
    catch (error) { set({ error: String(error) }); }
  },
  applyProgress: (progress) => {
    const current = get().progress;
    if (get().operation === "pick" && current?.category === progress.category && current.transport === progress.transport) {
      set({ progress });
    }
  },
}));
