import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Zapret2ScenarioProgress, Zapret2ScenarioResult } from "../../lib/tauri";
import { Zapret2ScenarioActions, Zapret2ScenarioResults, Zapret2ScenarioTrial } from "./Zapret2ScenarioTrial";

const progress: Zapret2ScenarioProgress = { requestId: "request", trialId: "trial", runId: "run", category: "youtube_twitch",
  transport: "quic", profileId: "candidate", scenario: "youtube_video", phase: "step", stepId: "playback",
  stepIndex: 2, total: 3, label: "Воспроизведение", instruction: "Проверь видео", confirmAfterUnixMs: Date.now() + 10000,
  expiresUnixMs: Date.now() + 180000 };

describe("directed application scenario", () => {
  it("requires real observation time and presents cancellation separately from a positive answer", () => {
    const markup = renderToStaticMarkup(<Zapret2ScenarioTrial progress={progress} cancelling={false} observing={false}
      observe={vi.fn()} cancel={vi.fn()} />);
    expect(markup).toMatch(/disabled=""[^>]*>Подтвердить через/);
    expect(markup).toContain("Не удалось проверить"); expect(markup).toContain("Отмена и возврат");
    expect(markup).toContain("Результат подтверждаешь ты");
    const restoring = renderToStaticMarkup(<Zapret2ScenarioTrial progress={{ ...progress, phase: "restoring" }}
      cancelling={false} observing={false} observe={vi.fn()} cancel={vi.fn()} />);
    expect(restoring).not.toContain("Шаг работает"); expect(restoring).not.toContain("Не работает");
  });
  it("only offers scenario kinds that fit the service", () => {
    expect(renderToStaticMarkup(<Zapret2ScenarioActions category="discord" transport="tls" disabled={false} test={vi.fn()} />)).toContain("Проверить голос");
    expect(renderToStaticMarkup(<Zapret2ScenarioActions category="discord" transport="quic" disabled={false} test={vi.fn()} />)).toBe("");
    expect(renderToStaticMarkup(<Zapret2ScenarioActions category="youtube_twitch" transport="quic" disabled={false} test={vi.fn()} />)).toContain("Проверить видео");
  });
  it("labels user reports about video without promoting them to QUIC or DPI execution evidence", () => {
    const result: Zapret2ScenarioResult = { ...progress, source: "user_report", scope: "whole_configuration", actualTransport: "unverified",
      configurationFingerprint: "configuration", status: "passed", restored: true, contextValid: true, recommendationAvailable: true,
      failure: null, elapsedMs: 12000, measurement: { runId: "trial", profileId: "candidate", fingerprint: "recipe", runtimeGeneration: 1,
        ordinal: 0, phase: "guided_scenario", transport: "quic", testerVersion: "guided", execution: "unverified",
        networkBefore: "network", networkAfter: "network", comparable: true, contextReason: null, functions: [] } };
    const markup = renderToStaticMarkup(<Zapret2ScenarioResults results={[result]} />);
    expect(markup).toContain("работает по твоей проверке"); expect(markup).toContain("Транспорт не определён");
    expect(markup).not.toContain("QUIC-подключение подтверждено"); expect(markup).not.toContain("Применить");
    const aborted = renderToStaticMarkup(<Zapret2ScenarioResults results={[{ ...result, status: "aborted",
      restored: false, contextValid: false, recommendationAvailable: false, failure: "Возврат runtime не подтверждён." }]} />);
    expect(aborted).toContain("Возврат runtime не подтверждён.");
  });
});
