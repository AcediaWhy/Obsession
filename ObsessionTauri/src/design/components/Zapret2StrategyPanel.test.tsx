import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Zapret2CatalogProfile, Zapret2PickerResult, Zapret2PickerProgress } from "../../lib/tauri";
import { Zapret2StrategyPanel } from "./Zapret2StrategyPanel";

const state = vi.hoisted(() => ({
  categories: ["discord", "youtube_twitch", "gaming"], transitioning: false, testing: false, active: false,
  catalog: [] as Zapret2CatalogProfile[], busy: false, loading: false,
  operation: null as "pick" | null, progress: null as Zapret2PickerProgress | null, cancelling: false,
  results: {} as Record<string, Zapret2PickerResult>, clienthello: true,
  load: vi.fn(), select: vi.fn(), pick: vi.fn(), applyResult: vi.fn(), cancel: vi.fn(), applyProgress: vi.fn(),
}));
vi.mock("../render", () => ({ useMotionOff: () => false, useRenderHidden: () => false }));
vi.mock("../../store/dpiStore", () => ({ useDpiStore: (select: (value: unknown) => unknown) => select({
  active: state.active, transitioning: state.transitioning, testing: state.testing, selectedCategories: state.categories,
}) }));
vi.mock("../../store/settingsStore", () => ({ useSettingsStore: (select: (value: unknown) => unknown) => select({
  settings: { zapret2_level: 4, zapret2_clienthello: state.clienthello, zapret2_pick_mode: "balanced" },
  protectedDpiAvailable: true, saving: false, error: "", patchConfirmed: vi.fn(),
}) }));
vi.mock("../../store/zapret2StrategyStore", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../store/zapret2StrategyStore")>();
  return { ...original, useZapret2StrategyStore: () => ({ ...state, error: "" }) };
});
vi.mock("../../lib/tauri", () => ({ runtime: { listen: vi.fn() } }));

const profile = (category: string, profileId: string, name: string, transport = "tls", selected = false,
  experimental = false): Zapret2CatalogProfile => ({ category, profileId, name, transport, selected,
  experimental, dataPlane: transport === "voice", level: 1 });

describe("independent Zapret2 service controls", () => {
  beforeEach(() => {
    state.categories = ["discord", "youtube_twitch", "gaming"];
    state.transitioning = false;
    state.testing = false;
    state.active = false;
    state.busy = false;
    state.operation = null;
    state.progress = null;
    state.cancelling = false;
    state.results = {};
    state.catalog = [
      profile("discord", "discord-normal", "Discord split", "tls", true),
      profile("discord", "discord-clone", "ClientHello · тест", "tls", false, true),
      profile("discord", "discord-quic", "Discord UDP", "quic", true),
      profile("discord", "discord-voice", "Discord voice", "voice", true),
      profile("youtube_twitch", "youtube-normal", "YouTube split", "tls", true),
      profile("youtube_twitch", "youtube-other", "YouTube multisplit"),
      profile("youtube_twitch", "youtube-quic", "YouTube UDP", "quic", true),
      profile("gaming", "gaming-normal", "Gaming split", "tls", true),
    ];
  });

  it("shows independent YouTube choices and per-service pickers", () => {
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    for (const label of ["Discord", "YouTube", "Gaming + GitHub"]) {
      expect(markup).toContain(`aria-label="Конфигурации ${label}"`);
      expect(markup).toContain(`aria-label="${label} · TLS"`);
    }
    expect(markup).toContain("YouTube multisplit");
    expect(markup).toContain('aria-label="YouTube · QUIC"');
    expect(markup).toContain('aria-label="Транспорт подбора YouTube"');
    expect(markup.match(/>Подобрать<\/button>/g)).toHaveLength(3);
    expect(markup).not.toMatch(/Набор [1-4]|Начать измерение|Локальный подбор/);
  });

  it("keeps research provenance separate from a local service result", () => {
    const entry = state.catalog[0];
    entry.experimental = true;
    entry.candidate = { label: "Window + disorder", family: "window_reorder", evidence: "author_report",
      urls: ["https://github.com/bol-van/zapret2/discussions/208"], notes: "Успех заявлен автором; голос не проверялся." };
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(markup).toContain('label="Исследовательские профили"');
    expect(markup).toContain("полное испытание сервиса не проведено");
    expect(markup).toContain("Условия и источник");
    expect(markup).toContain(entry.candidate.urls[0]);
    expect(markup).not.toContain("Веб-проверки пройдены");
  });

  it("uses the selected normal profile even when the retired global ClientHello flag is true", () => {
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(markup).toMatch(/<option[^>]*value="discord-normal" selected="">Discord split<\/option>/);
    expect(markup).toMatch(/<optgroup label="Эксперимент"><option[^>]*value="discord-clone">ClientHello · тест<\/option><\/optgroup>/);
    expect(markup).not.toMatch(/<option[^>]*value="discord-clone" selected=""/);
  });

  it("does not claim voice or video works when only web checks passed", () => {
    state.results = { "discord:tls": { category: "discord", transport: "tls", profileId: "discord-normal",
      checked: 1, total: 2, status: "passed", report: { passed: true, status: "passed", checks: [{
        url: "https://discord.com/api/v10/gateway", passed: true, elapsed_ms: 80, bytes: 100, attempts: 1, http_status: 200, error: null,
      }] } } };
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(markup).toContain("Все проверки не подтверждены");
    expect(markup).toMatch(/Голос<\/span><span[^>]*>Не проверен/);
    expect(markup).not.toContain("Применить TLS");
    state.results["discord:tls"].report!.status = "partial";
    const partial = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(partial).not.toContain("Применить TLS");
    expect(partial).toContain("Все проверки не подтверждены");
  });

  it("allows manual changes with active protection and locks all controls during a picker", () => {
    state.active = true;
    expect(renderToStaticMarkup(<Zapret2StrategyPanel />)).not.toContain('disabled=""');
    state.testing = true;
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(markup.match(/<select[^>]*disabled=""/g)).toHaveLength(9);
    expect(markup.match(/<button[^>]*disabled=""/g)).toHaveLength(5);
  });

  it("only renders the selected services", () => {
    state.categories = ["youtube_twitch"];
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(markup).toContain("YouTube multisplit");
    expect(markup).not.toContain('aria-label="Конфигурации Discord"');
    expect(markup).not.toContain('aria-label="Конфигурации Gaming + GitHub"');
  });

  it("only presents the animation for an active picker and keeps cancellation locked during restoration", () => {
    expect(renderToStaticMarkup(<Zapret2StrategyPanel />)).not.toContain('role="progressbar"');
    state.busy = true;
    state.operation = "pick";
    state.progress = { category: "discord", transport: "tls", current: 2, total: 5,
      profileId: "discord-normal", label: "Discord split", phase: "screening" };
    const picking = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(picking.match(/role="progressbar"/g)).toHaveLength(1);
    expect(picking).toContain('aria-label="Подбор Discord"');
    expect(picking).toContain('aria-valuenow="2"');
    expect(picking).toContain("Подбираем TLS…");
    const cancelMarkup = picking.match(/<button[^>]*aria-label="Отмена и возврат Discord"[^>]*>Отмена<\/button>/)?.[0];
    expect(cancelMarkup).toBeDefined();
    expect(cancelMarkup).not.toContain('disabled=""');
    state.progress.phase = "restoring";
    const restoring = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(restoring).toContain("Возвращаем исходный профиль…");
    expect(restoring).toMatch(/<button[^>]*disabled=""[^>]*>Отмена<\/button>/);
    state.busy = false;
    state.operation = null;
    expect(renderToStaticMarkup(<Zapret2StrategyPanel />)).not.toContain('role="progressbar"');
  });

  it("keeps one shared depth setting next to each picker and collapses profile details", () => {
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    for (const label of ["Discord", "YouTube", "Gaming + GitHub"]) {
      expect(markup).toContain(`aria-label="Глубина проверки для ${label}"`);
    }
    expect(markup.match(/value="balanced" selected=""/g)).toHaveLength(3);
    expect(markup.match(/<details class="z2-service-details">/g)).toHaveLength(3);
    expect(markup).not.toContain('aria-label="Режим подбора"');
  });

  it("describes a QUIC handshake without claiming a page or video was loaded", () => {
    state.results = { "youtube_twitch:quic": { category: "youtube_twitch", transport: "quic", profileId: "youtube-quic",
      checked: 1, total: 2, status: "passed", report: { passed: true, status: "passed", checks: [{
        url: "https://www.youtube.com", passed: true, elapsed_ms: 80, bytes: 0, attempts: 1, http_status: null, error: null,
      }] } } };
    state.results["youtube_twitch:quic"].tournament = { runId: "quic-run", requiredConfirmations: 2,
      maxAttempts: 3, budgetMs: 90000, maxBodyBytes: 0, elapsedMs: 300, bytesRead: 0, stopReason: "completed", rounds: [],
      candidates: [{ profileId: "youtube-quic", label: "QUIC", family: "fake", fingerprint: "fixed", rounds: 2,
        completeRounds: 2, requiredConfirmations: 2, status: "confirmed", bytes: 0,
        functions: [{ function: "quic_0", label: "QUIC YouTube", required: true, passed: 2, failed: 0,
          inconclusive: 0, cancelled: 0, notTested: 0, proof: "quic_handshake" }] }] };
    const markup = renderToStaticMarkup(<Zapret2StrategyPanel />);
    expect(markup).toContain("QUIC-подключение подтверждено");
    expect(markup).toContain("HTTP/3-запросы и видеопоток не проверяются");
    expect(markup).not.toContain("Страница YouTube");
    expect(markup).not.toContain("загружено");
    expect(markup).toContain("Применить QUIC");
  });
});
