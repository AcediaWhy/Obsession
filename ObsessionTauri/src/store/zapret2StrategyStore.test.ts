import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DpiTestReport, Zapret2CatalogProfile, Zapret2PickerResult, Zapret2ScenarioResult } from "../lib/tauri";

const mocks = vi.hoisted(() => ({
  history: vi.fn(), scenario: vi.fn(), observe: vi.fn(), catalog: vi.fn(), select: vi.fn(), apply: vi.fn(), pick: vi.fn(), cancel: vi.fn(), bootstrap: vi.fn(),
  profiles: vi.fn(), dpiStatus: vi.fn(), settings: vi.fn(), legacyTest: vi.fn(), recordWorkingConfig: vi.fn(),
}));
vi.mock("../lib/tauri", () => ({
  api: { dpiZapret2History: mocks.history, dpiZapret2TestScenario: mocks.scenario, dpiZapret2ScenarioObserve: mocks.observe, dpiZapret2Catalog: mocks.catalog, dpiZapret2Select: mocks.select, dpiZapret2Pick: mocks.pick,
    dpiZapret2ApplyResult: mocks.apply, dpiTestCancel: mocks.cancel, dpiTest: mocks.legacyTest, recordWorkingConfig: mocks.recordWorkingConfig },
  runtime: { bootstrap: mocks.bootstrap },
}));
vi.mock("./dpiStore", async () => {
  const { create } = await import("zustand");
  return { useDpiStore: create(() => ({ active: false, transitioning: false, testing: false,
    testCancel: false, testingLabel: "", selectedCategories: ["discord", "youtube_twitch", "gaming"],
    loadZapret2Profiles: mocks.profiles, applyVersionedStatus: mocks.dpiStatus })) };
});
vi.mock("./settingsStore", async () => {
  const { create } = await import("zustand");
  return { useSettingsStore: create(() => ({ protectedDpiAvailable: true, applyBootstrap: mocks.settings })) };
});
import { useDpiStore } from "./dpiStore";
import { useSettingsStore } from "./settingsStore";
import { applicablePickerResult, useZapret2StrategyStore } from "./zapret2StrategyStore";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const profile = (category: string, profileId: string, transport = "tls", selected = false): Zapret2CatalogProfile => ({
  category, profileId, name: profileId, transport, experimental: false, dataPlane: transport === "voice", level: 1, selected,
});
const originalCatalog = [
  profile("discord", "discord-normal", "tls", true), profile("discord", "discord-other"),
  profile("discord", "discord-voice", "voice", true), profile("youtube_twitch", "youtube-normal", "tls", true),
  profile("youtube_twitch", "youtube-other"), profile("youtube_twitch", "youtube-quic", "quic", true),
  profile("gaming", "gaming-normal", "tls", true),
];
const report: DpiTestReport = { passed: true, status: "passed", checks: [{
  url: "https://discord.com/api/v10/gateway", passed: true, elapsed_ms: 120, bytes: 128, attempts: 1, http_status: 200, error: null,
}] };
const passed: Zapret2PickerResult = {
  category: "discord", transport: "tls", profileId: "discord-other", status: "passed", checked: 2, total: 2, report,
  tournament: { runId: "run", requiredConfirmations: 2, maxAttempts: 3, budgetMs: 90000, maxBodyBytes: 1024,
    elapsedMs: 1000, bytesRead: 256, stopReason: "completed", rounds: [1,2].map((ordinal) => ({
      runId:"run",profileId:"discord-other",fingerprint:"fixed",runtimeGeneration:ordinal,ordinal,phase:"confirmation",
      transport:"tls",testerVersion:"zapret2-functions-v5-routing",execution:"flow_routing_verified",
      networkBefore:"network",networkAfter:"network",comparable:true,contextReason:null,
      functions:[{function:"api",label:"API",required:true,outcome:"passed",proof:"http_document",stage:"complete",
        reason:null,target:"https://discord.com/api/v10/gateway",remoteAddress:"203.0.113.1:443",ipFamily:"ipv4",
        elapsedMs:120,bytes:128,httpStatus:200,connections:[{purpose:"unit_fixture",host:"discord.com",
          localAddress:`192.0.2.1:${50000+ordinal}`,remoteAddress:"203.0.113.1:443",expectedProfileId:"discord-other",
          observedProfiles:["discord-other"],observedActions:["fixture"],observedSends:["fixture"],status:"verified"}]}],
    })), candidates: [{
      profileId: "discord-other", label: "Other", family: "split", fingerprint: "fixed", rounds: 2, completeRounds: 2,
      requiredConfirmations: 2, routingVerifiedRounds: 2, status: "confirmed", bytes: 256, medianMs: 120, functions: [{
        function: "api", label: "API", required: true, passed: 2, failed: 0, inconclusive: 0, notTested: 0, cancelled: 0, proof: "http_document",
      }],
    }] },
};

function scenarioResult(requestId: string, overrides: Partial<Zapret2ScenarioResult> = {}): Zapret2ScenarioResult {
  return { requestId, trialId: "trial", runId: "run", category: "discord", transport: "tls", profileId: "discord-other",
    scenario: "discord_startup", source: "user_report", scope: "whole_configuration", actualTransport: "unverified",
    configurationFingerprint: "whole-configuration", status: "passed", restored: true, contextValid: true,
    recommendationAvailable: true, failure: null, elapsedMs: 1000,
    measurement: { runId: "trial", profileId: "discord-other", fingerprint: "fixed", runtimeGeneration: 4, ordinal: 0,
      phase: "guided_scenario", transport: "tls", testerVersion: "zapret2-guided-scenarios-v1", execution: "unverified",
      networkBefore: "network", networkAfter: "network", comparable: true, contextReason: null, functions: [] }, ...overrides };
}

describe("per-service Zapret2 selection and finite picker", () => {
  it("tests an explicit Discord pair without applying it or inheriting donor success", async () => {
    const pair={apiProfileId:"discord-normal",cdnProfileId:"discord-other"};
    useZapret2StrategyStore.setState({catalog:originalCatalog.map((entry)=>({...entry,compositionEligible:entry.category==="discord"&&entry.transport==="tls"}))});
    const pending=deferred<Zapret2PickerResult>(); mocks.pick.mockReturnValue(pending.promise);
    const testing=useZapret2StrategyStore.getState().pick("discord","tls","balanced",pair);
    expect(mocks.pick.mock.calls).toEqual([["discord","tls","balanced",pair]]);
    expect(useZapret2StrategyStore.getState().progress?.phase).toBe("composition");
    expect(useDpiStore.getState().testing).toBe(true);
    const id="dc1_discord-normal--discord-other";
    pending.resolve({...passed,profileId:id,tournament:{...passed.tournament!,candidates:[...passed.tournament!.candidates,
      {...passed.tournament!.candidates[0],profileId:id,composition:pair,status:"not_tested",rounds:0,completeRounds:0,functions:[]}]}});
    await testing;
    await useZapret2StrategyStore.getState().applyResult("discord","tls");
    expect(mocks.apply).not.toHaveBeenCalled();
    expect(useDpiStore.getState().testing).toBe(false);
    expect(useZapret2StrategyStore.getState().error).toContain("не подтверждены");
  });
  it("rejects unsupported duplicate or noneligible composition parents before probing", async () => {
    for (const pair of [{apiProfileId:"discord-normal",cdnProfileId:"discord-normal"},
      {apiProfileId:"discord-normal",cdnProfileId:"youtube-other"},
      {apiProfileId:"discord-normal",cdnProfileId:"missing"}]) {
      await useZapret2StrategyStore.getState().pick("discord","tls","fast",pair);
    }
    expect(mocks.pick).not.toHaveBeenCalled(); expect(useDpiStore.getState().testing).toBe(false);
  });
  it("does not recreate active recommendations from archived trials", async () => {
    mocks.history.mockResolvedValue([{ recordedAtUnixMs: Date.now(), kind: "tournament", subject: "Old tournament", outcome: "passed", report: passed }]);
    await useZapret2StrategyStore.getState().loadHistory();
    expect(useZapret2StrategyStore.getState().history).toHaveLength(1);
    expect(useZapret2StrategyStore.getState().results).toEqual({});
    await useZapret2StrategyStore.getState().applyResult("discord", "tls"); expect(mocks.apply).not.toHaveBeenCalled();
  });
  it("waits for application observations and preserves a newer step when the acknowledgement arrives late", async () => {
    const pending = deferred<Zapret2ScenarioResult>(); mocks.scenario.mockReturnValue(pending.promise);
    useZapret2StrategyStore.setState({ results: { "discord:tls": passed } });
    const testing = useZapret2StrategyStore.getState().testScenario("discord", "tls", "discord-other", "discord_startup");
    const initial = useZapret2StrategyStore.getState().scenarioProgress!;
    expect(useDpiStore.getState().testing).toBe(true);
    const step = { ...initial, trialId: "trial", phase: "step" as const, stepId: "fresh_client", stepIndex: 1, total: 2,
      label: "Fresh client", expiresUnixMs: Date.now() + 180000 };
    useZapret2StrategyStore.getState().applyScenarioProgress({ ...step, requestId: "old" });
    expect(useZapret2StrategyStore.getState().scenarioProgress).toEqual(initial);
    useZapret2StrategyStore.getState().applyScenarioProgress(step);
    const acknowledgement = deferred<void>(); mocks.observe.mockReturnValue(acknowledgement.promise);
    const observing = useZapret2StrategyStore.getState().observeScenario("passed");
    expect(mocks.observe.mock.calls).toEqual([["trial", "fresh_client", "passed"]]);
    useZapret2StrategyStore.getState().applyScenarioProgress({ ...step, stepId: "channels", stepIndex: 2 });
    acknowledgement.resolve(); await observing;
    expect(useZapret2StrategyStore.getState().scenarioProgress?.stepId).toBe("channels");
    pending.resolve(scenarioResult(initial.requestId)); await testing;
    expect(useZapret2StrategyStore.getState().busy).toBe(false);
    expect(useDpiStore.getState().testing).toBe(false);
    expect(useZapret2StrategyStore.getState().results["discord:tls"].tournament?.candidates[0].completeRounds).toBe(2);
    expect(Object.values(useZapret2StrategyStore.getState().scenarios)[0].source).toBe("user_report");
    expect(mocks.apply).not.toHaveBeenCalled(); expect(mocks.recordWorkingConfig).not.toHaveBeenCalled();
  });

  it("blocks application after a failed primary scenario and permits an explicit retest", async () => {
    useZapret2StrategyStore.setState({ results: { "discord:tls": passed } });
    mocks.scenario.mockImplementation(async (requestId: string) => scenarioResult(requestId, { status: "failed", recommendationAvailable: false }));
    await useZapret2StrategyStore.getState().testScenario("discord", "tls", "discord-other", "discord_startup");
    await useZapret2StrategyStore.getState().applyResult("discord", "tls"); expect(mocks.apply).not.toHaveBeenCalled();
    mocks.scenario.mockImplementation(async (requestId: string) => scenarioResult(requestId));
    await useZapret2StrategyStore.getState().testScenario("discord", "tls", "discord-other", "discord_startup");
    await useZapret2StrategyStore.getState().applyResult("discord", "tls");
    expect(mocks.apply.mock.calls).toEqual([["run", "discord", "tls", "discord-other"]]);
  });

  it("cannot turn a late successful scenario response into a success after cancellation", async () => {
    const pending = deferred<Zapret2ScenarioResult>(); mocks.scenario.mockReturnValue(pending.promise);
    useZapret2StrategyStore.setState({ results: { "discord:tls": passed } });
    const testing = useZapret2StrategyStore.getState().testScenario("discord", "tls", "discord-other", "discord_startup");
    const initial = useZapret2StrategyStore.getState().scenarioProgress!;
    await useZapret2StrategyStore.getState().cancel();
    useZapret2StrategyStore.getState().applyScenarioProgress({ ...initial, requestId: "old", trialId: "old" });
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
    const accepted = { ...initial, trialId: "trial" };
    useZapret2StrategyStore.getState().applyScenarioProgress(accepted);
    useZapret2StrategyStore.getState().applyScenarioProgress(accepted);
    expect(mocks.cancel).toHaveBeenCalledTimes(2);
    await useZapret2StrategyStore.getState().observeScenario("passed"); expect(mocks.observe).not.toHaveBeenCalled();
    pending.resolve(scenarioResult(initial.requestId)); await testing;
    expect(Object.values(useZapret2StrategyStore.getState().scenarios)[0].status).toBe("cancelled");
    expect(mocks.cancel).toHaveBeenCalledTimes(2); expect(mocks.apply).not.toHaveBeenCalled();
  });

  it.each(["context", "identity"])("discards recommendations when the scenario loses its %s", async (failure) => {
    useZapret2StrategyStore.setState({ results: { "discord:tls": passed } });
    mocks.scenario.mockImplementation(async (requestId: string) => scenarioResult(requestId, failure === "context"
      ? { contextValid: false, restored: false, status: "aborted" } : { requestId: "another-request" }));
    await useZapret2StrategyStore.getState().testScenario("discord", "tls", "discord-other", "discord_startup");
    expect(useZapret2StrategyStore.getState().results).toEqual({}); expect(useZapret2StrategyStore.getState().busy).toBe(false);
  });
  it("cannot apply one-shot, cancelled, aborted or insufficient evidence", () => {
    expect(applicablePickerResult({ ...passed, tournament: undefined })).toBe(false);
    for (const status of ["cancelled", "aborted", "insufficient", "partial", "inconclusive"] as const) {
      expect(applicablePickerResult({ ...passed, status })).toBe(false);
    }
    expect(applicablePickerResult({ ...passed, tournament: { ...passed.tournament!, candidates: [
      { ...passed.tournament!.candidates[0], rounds: 1, completeRounds: 1 },
    ] } })).toBe(false);
  });

  it("rejects green application checks without exact fresh engine routing", () => {
    expect(applicablePickerResult(passed)).toBe(true);
    for (const mutation of ["old_tester","missing_route","missing_native_send","wrong_block","copied_generation","wrong_run","old_summary"] as const) {
      const result=structuredClone(passed); const tournament=result.tournament!;
      const round=tournament.rounds[1];
      if (mutation==="old_tester") round.testerVersion="zapret2-functions-v4";
      if (mutation==="missing_route") round.functions[0].connections=[];
      if (mutation==="missing_native_send") delete round.functions[0].connections![0].observedSends;
      if (mutation==="wrong_block") round.functions[0].connections![0].observedProfiles=["original-working-config"];
      if (mutation==="copied_generation") round.runtimeGeneration=tournament.rounds[0].runtimeGeneration;
      if (mutation==="wrong_run") round.runId="previous-run";
      if (mutation==="old_summary") delete tournament.candidates[0].routingVerifiedRounds;
      expect(applicablePickerResult(result),mutation).toBe(false);
    }
  });

  it("applies an explicitly chosen tied candidate through the result receipt", async () => {
    useZapret2StrategyStore.setState({ results: { "discord:tls": { ...passed, status: "tie", profileId: undefined } } });
    await useZapret2StrategyStore.getState().applyResult("discord", "tls", "discord-other");
    expect(mocks.apply.mock.calls).toEqual([["run", "discord", "tls", "discord-other"]]);
    expect(mocks.select).not.toHaveBeenCalled();
    expect(useZapret2StrategyStore.getState().results).toEqual({});
  });
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.history.mockResolvedValue([]);
    mocks.catalog.mockResolvedValue(originalCatalog);
    mocks.select.mockResolvedValue(undefined);
    mocks.cancel.mockResolvedValue(undefined);
    mocks.profiles.mockResolvedValue(undefined);
    mocks.bootstrap.mockResolvedValue({ dpi: { revision: 2 }, settings: { revision: 2 } });
    mocks.pick.mockResolvedValue(passed);
    useDpiStore.setState({ active: false, transitioning: false, testing: false, testCancel: false, testingLabel: "",
      selectedCategories: ["discord", "youtube_twitch", "gaming"] });
    useSettingsStore.setState({ protectedDpiAvailable: true });
    useZapret2StrategyStore.setState({ catalog: originalCatalog, loading: false, busy: false, operation: null,
      cancelling: false, progress: null, results: {}, history: [], historyError: "", scenarios: {}, scenarioProgress: null, observing: false, error: "" });
  });

  it("changes YouTube TLS independently and refreshes authoritative selection while DPI is active", async () => {
    useDpiStore.setState({ active: true });
    const after = originalCatalog.map((entry) => entry.category === "youtube_twitch" && entry.transport === "tls"
      ? { ...entry, selected: entry.profileId === "youtube-other" } : entry);
    mocks.catalog.mockResolvedValue(after);
    await useZapret2StrategyStore.getState().select("youtube_twitch", "tls", "youtube-other");
    expect(mocks.select.mock.calls).toEqual([["youtube_twitch", "tls", "youtube-other"]]);
    expect(useZapret2StrategyStore.getState().catalog.filter((entry) => entry.selected).map((entry) => entry.profileId))
      .toEqual(["discord-normal", "discord-voice", "youtube-other", "youtube-quic", "gaming-normal"]);
    expect(mocks.settings).toHaveBeenCalledWith({ revision: 2 });
    expect(useDpiStore.getState().transitioning).toBe(false);
  });

  it("returns a passed candidate without selecting it until Apply", async () => {
    await useZapret2StrategyStore.getState().pick("discord", "tls", "balanced");
    expect(mocks.pick.mock.calls).toEqual([["discord", "tls", "balanced"]]);
    expect(mocks.select).not.toHaveBeenCalled();
    expect(useZapret2StrategyStore.getState().catalog.find((entry) => entry.profileId === "discord-normal")?.selected).toBe(true);
    expect(applicablePickerResult(useZapret2StrategyStore.getState().results["discord:tls"])).toBe(true);
    await useZapret2StrategyStore.getState().applyResult("discord", "tls");
    expect(mocks.apply.mock.calls).toEqual([["run", "discord", "tls", "discord-other"]]);
    expect(mocks.select).not.toHaveBeenCalled();
    expect(mocks.legacyTest).not.toHaveBeenCalled();
    expect(mocks.recordWorkingConfig).not.toHaveBeenCalled();
  });

  it("keeps the testing guard until rollback finishes and discards a success racing cancellation", async () => {
    const pending = deferred<Zapret2PickerResult>();
    mocks.pick.mockReturnValue(pending.promise);
    const picking = useZapret2StrategyStore.getState().pick("discord", "tls", "deep");
    await useZapret2StrategyStore.getState().cancel();
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
    expect(useDpiStore.getState().testing).toBe(true);
    expect(useZapret2StrategyStore.getState().busy).toBe(true);
    await useZapret2StrategyStore.getState().select("youtube_twitch", "tls", "youtube-other");
    expect(mocks.select).not.toHaveBeenCalled();
    pending.resolve(passed);
    await picking;
    const result = useZapret2StrategyStore.getState().results["discord:tls"];
    expect(result.status).toBe("cancelled");
    expect(result.profileId).toBeUndefined();
    expect(useDpiStore.getState().testing).toBe(false);
    await useZapret2StrategyStore.getState().applyResult("discord", "tls");
    expect(mocks.select).not.toHaveBeenCalled();
    expect(mocks.recordWorkingConfig).not.toHaveBeenCalled();
  });

  it.each([
    { ...report, status: "partial" as const, passed: false },
    { ...report, checks: [{ ...report.checks[0], passed: false }] },
    { ...report, checks: [] },
    undefined,
  ])("does not offer partial or missing evidence as a passed configuration", async (evidence) => {
    mocks.pick.mockResolvedValue({ ...passed, report: evidence });
    await useZapret2StrategyStore.getState().pick("discord", "tls", "fast");
    expect(applicablePickerResult(useZapret2StrategyStore.getState().results["discord:tls"])).toBe(false);
    expect(useZapret2StrategyStore.getState().error).toContain("не подтверждены");
    await useZapret2StrategyStore.getState().applyResult("discord", "tls");
    expect(mocks.select).not.toHaveBeenCalled();
  });

  it("allows YouTube QUIC auto-pick while Discord voice and QUIC remain manual", async () => {
    mocks.pick.mockResolvedValue({ ...passed, category: "youtube_twitch", transport: "quic", profileId: "youtube-quic" });
    await useZapret2StrategyStore.getState().pick("discord", "voice", "fast");
    await useZapret2StrategyStore.getState().pick("discord", "quic", "fast");
    await useZapret2StrategyStore.getState().pick("gaming", "quic", "fast");
    expect(mocks.pick).not.toHaveBeenCalled();
    await useZapret2StrategyStore.getState().pick("youtube_twitch", "quic", "balanced");
    expect(mocks.pick.mock.calls).toEqual([["youtube_twitch", "quic", "balanced"]]);
  });

  it("rejects cross-service responses and ignores unrelated progress", async () => {
    const pending = deferred<Zapret2PickerResult>();
    mocks.pick.mockReturnValue(pending.promise);
    const picking = useZapret2StrategyStore.getState().pick("discord", "tls", "fast");
    const initial = useZapret2StrategyStore.getState().progress;
    useZapret2StrategyStore.getState().applyProgress({ category: "youtube_twitch", transport: "tls", current: 1, total: 2,
      profileId: "youtube-other", label: "test", phase: "testing" });
    expect(useZapret2StrategyStore.getState().progress).toEqual(initial);
    pending.resolve({ ...passed, category: "youtube_twitch" });
    await picking;
    expect(useZapret2StrategyStore.getState().results).toEqual({});
    expect(useZapret2StrategyStore.getState().error).toContain("другому сервису");
  });

  it("keeps the newest catalog when an earlier request arrives late", async () => {
    const older = deferred<Zapret2CatalogProfile[]>();
    mocks.catalog.mockReturnValueOnce(older.promise).mockResolvedValueOnce([profile("gaming", "gaming-normal", "tls", true)]);
    const loading = useZapret2StrategyStore.getState().load(["discord"]);
    await useZapret2StrategyStore.getState().load(["gaming"]);
    older.resolve(originalCatalog);
    await loading;
    expect(useZapret2StrategyStore.getState().catalog.map((entry) => entry.category)).toEqual(["gaming"]);
  });
});
