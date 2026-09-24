import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OnboardingSnapshot, OnboardingFailure } from "../../lib/onboarding";

const state = vi.hoisted(() => ({
  snapshot: null as OnboardingSnapshot | null,
  failure: null as OnboardingFailure | null,
  busy: false,
  refresh: vi.fn(), verify: vi.fn(), rollback: vi.fn(), complete: vi.fn(), launchRepair: vi.fn(),
}));
const settings = vi.hoisted(() => ({ loaded: true, protectedRuntime: { serviceAvailable: true } }));
vi.mock("../../store/onboardingStore", () => ({ useOnboardingStore: (select: (s: typeof state) => unknown) => select(state) }));
vi.mock("../../store/settingsStore", () => ({ useSettingsStore: (select: (s: typeof settings) => unknown) => select(settings) }));
vi.mock("../../store/launcherBootstrap", () => ({ launcherBootstrap: { refresh: vi.fn() } }));
import { Onboarding } from "./Onboarding";

function pending(status: "applied" | "recovery_required" | "rolled_back"): OnboardingSnapshot {
  return {
    flowVersion: 2, revision: 1, phase: "recovery_required", presentation: "required",
    draft: { goals: { dpi: true, ai: false, telegram: false }, dpiEngine: "legacy", aiProvider: "malw" },
    plan: null, transaction: { transactionId: "tx", planId: "plan", status, checkpoint: "test", verification: null },
    verification: null, terminalStatus: "active", destination: null,
  };
}

describe("legacy setup recovery notice", () => {
  beforeEach(() => {
    state.snapshot = null;
    state.failure = null;
    state.busy = false;
    settings.loaded = true;
    settings.protectedRuntime.serviceAvailable = true;
  });
  it("renders nothing on a fresh installation", () => {
    expect(renderToStaticMarkup(<Onboarding />)).toBe("");
  });
  it("ignores old welcome and offer states without transactions", () => {
    for (const presentation of ["required", "offer", "modal"] as const) {
      state.snapshot = { ...pending("applied"), phase: "welcome", presentation, transaction: null };
      expect(renderToStaticMarkup(<Onboarding />)).toBe("");
    }
  });
  it("shows interrupted recovery without a modal or automatic mutation", () => {
    state.snapshot = pending("recovery_required");
    const html = renderToStaticMarkup(<Onboarding />);
    expect(html).toContain("Отменить прежнюю настройку");
    expect(html).not.toContain('role="dialog"');
    expect(html).not.toContain("Оставить применённые настройки");
    expect(state.rollback).not.toHaveBeenCalled();
  });
  it("requires verification before offering to keep applied changes", () => {
    state.snapshot = pending("applied");
    expect(renderToStaticMarkup(<Onboarding />)).not.toContain("Оставить применённые настройки");
    state.snapshot.transaction!.verification = { outcome: "partial", targets: [], accepted: false };
    expect(renderToStaticMarkup(<Onboarding />)).toContain("Оставить применённые настройки");
  });
  it("hides completed operations and allows closing a finished rollback", () => {
    state.snapshot = pending("rolled_back");
    expect(renderToStaticMarkup(<Onboarding />)).toContain("Закрыть уведомление");
    state.snapshot.terminalStatus = "completed";
    expect(renderToStaticMarkup(<Onboarding />)).toBe("");
  });
  it("offers repair only after service status has loaded", () => {
    settings.protectedRuntime.serviceAvailable = false;
    settings.loaded = false;
    expect(renderToStaticMarkup(<Onboarding />)).toBe("");
    settings.loaded = true;
    expect(renderToStaticMarkup(<Onboarding />)).toContain("Восстановить службу");
  });
  it("keeps an unreadable journal visible with a retry action", () => {
    state.failure = { code: "PREFLIGHT_FAILED", retryable: true, messageCode: "state_read", logPath: null };
    const html = renderToStaticMarkup(<Onboarding />);
    expect(html).toContain("Не удалось прочитать сохранённую настройку");
    expect(html).toContain("Проверить снова");
  });
});
