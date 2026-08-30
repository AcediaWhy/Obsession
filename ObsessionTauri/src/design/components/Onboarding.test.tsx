import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  CORE_HERO_MIN_SIZE: 160,
  createRenderLoop: () => ({
    start: () => {},
    stop: () => {},
    setPaused: () => {},
    invalidate: () => {},
    dispose: () => {},
  }),
  frameQualityScale: () => 1,
  onRenderActiveChange: () => () => {},
  renderActive: () => false,
  useMotionOff: () => true,
  useRenderActive: () => false,
  useRenderHidden: () => false,
}));

vi.mock("../../lib/tauri", () => ({
  api: {
    updateSettings: vi.fn(),
    getSettings: vi.fn(),
  },
  win: {
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
  },
}));

const { onboardingMockState } = vi.hoisted(() => ({
  onboardingMockState: {
    loaded: true,
    busy: false,
    snapshot: {
      flowVersion: 2,
      revision: 1,
      phase: "welcome",
      presentation: "required",
      draft: {
        goals: { dpi: true, ai: false, telegram: false },
        dpiEngine: "legacy",
        aiProvider: "malw",
      },
      plan: null,
      transaction: null,
      verification: null,
      terminalStatus: "active",
      destination: null,
    },
    readiness: null,
    failure: null,
    checkReadiness: vi.fn(),
    saveDraft: vi.fn(),
    buildPlan: vi.fn(),
    apply: vi.fn(),
    verify: vi.fn(),
    acceptVerification: vi.fn(),
    rollback: vi.fn(),
    complete: vi.fn(),
    skip: vi.fn(),
    cancel: vi.fn(),
    launchRepair: vi.fn(),
    clearFailure: vi.fn(),
  },
}));

vi.mock("../../store/onboardingStore", () => ({
  useOnboardingStore: (selector: (state: typeof onboardingMockState) => unknown) =>
    selector(onboardingMockState),
}));

import type { Settings } from "../../lib/tauri";
import { useSecretStore } from "../../store/secretStore";
import { useSettingsStore } from "../../store/settingsStore";
import { useThemeStore } from "../../store/themeStore";
import {
  getOnboardingPrimaryButtonClass,
  getOnboardingKeyboardIntent,
  Onboarding,
} from "./Onboarding";
import { ThemePreview } from "./ThemePreview";

const settings: Settings = {
  minimize_to_tray: true,
  start_minimized: false,
  selected_categories: ["discord"],
  zapret2_selected_categories: ["discord"],
  selected_configs: {},
  proxy_port: 1443,
  fake_tls_domain: "",
  ai_provider: "malw",
  has_completed_onboarding: false,
  auto_recovery: false,
  legacy_reliability_migration_version: 1,
  legacy_reliability_enabled: true,
  legacy_reliability_mode: "observe_only",
  legacy_automatic_paused: true,
  legacy_reliability_frozen_categories: [],
  reduce_motion: false,
  hotkey_toggle: "Ctrl+Shift+KeyO",
  lan_publish_secs: 0,
  dpi_engine: "legacy",
  zapret2_level: 0,
  adaptive_strategy_enabled: false,
  adaptive_search_mode: "balanced",
};

function containsNestedButton(markup: string): boolean {
  let depth = 0;
  for (const match of markup.matchAll(/<\/?button\b/g)) {
    if (match[0].startsWith("</")) {
      depth = Math.max(0, depth - 1);
    } else {
      if (depth > 0) return true;
      depth += 1;
    }
  }
  return false;
}

describe("Onboarding PR 1 contract", () => {
  beforeEach(() => {
    useSettingsStore.setState({
      revision: 1,
      loaded: true,
      saving: false,
      saved: false,
      settings,
      elevated: false,
      protectedRuntimeAvailable: false,
      protectedDpiAvailable: false,
      protectedLegacyReliabilityAvailable: false,
      autostart: false,
      error: "",
    });
    useThemeStore.setState({ theme: "aurora" });
    useSecretStore.setState({ unlocked: [] });
  });

  it("does not turn Enter or arrow keys into global step navigation", () => {
    expect(getOnboardingKeyboardIntent("Enter", false, false)).toBeNull();
    expect(getOnboardingKeyboardIntent("ArrowRight", false, false)).toBeNull();
    expect(getOnboardingKeyboardIntent("ArrowLeft", false, false)).toBeNull();
    expect(getOnboardingKeyboardIntent("Escape", false, false)).toBe("open_confirmation");
    expect(getOnboardingKeyboardIntent("Escape", true, false)).toBe("close_confirmation");
    expect(getOnboardingKeyboardIntent("Escape", false, true)).toBeNull();
  });

  it("renders a named functional modal without nested interactive controls", () => {
    const markup = renderToStaticMarkup(<Onboarding />);

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain("Без повторного UAC");
    expect(markup).toContain("До экрана Review");
    expect(markup).toContain("hover:!bg-accent/90");
    expect(markup).toContain("!text-[#05060B]");
    expect(markup).toContain("focus-visible:!ring-accent-cyan");
    expect(markup).toContain('tabindex="-1"');
    expect(markup).toContain("text-ink outline-none");
    expect(containsNestedButton(markup)).toBe(false);
  });

  it("uses a dark primary-button foreground on light theme accents", () => {
    expect(getOnboardingPrimaryButtonClass("ophanim")).toContain("!text-[#05060B]");
    expect(getOnboardingPrimaryButtonClass("obsession")).toContain("!text-[#05060B]");
    expect(getOnboardingPrimaryButtonClass("ophanim")).not.toContain("!text-white");
    expect(getOnboardingPrimaryButtonClass("aurora")).toContain("!text-white");
  });

  it("keeps original core renderers decorative and outside the accessibility tree", () => {
    const markup = renderToStaticMarkup(
      <ThemePreview theme="ophanim" selected size={104} />,
    );

    expect(markup).toContain('aria-hidden="true"');
    expect(markup).not.toContain("<button");
    expect(markup).toContain("<svg");
    expect(markup).toContain("data-ophanim-cat-core");
    expect(markup).not.toContain("tabindex");
  });

  it("renders the Obsession theme tile without a nested button", () => {
    const markup = renderToStaticMarkup(
      <ThemePreview theme="obsession" selected size={104} />,
    );
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).toContain("data-choir-seal");
    expect(containsNestedButton(markup)).toBe(false);
    expect(markup).not.toContain("<button");
  });

  it("presents an interrupted transaction as a retryable safe rollback", () => {
    Object.assign(onboardingMockState.snapshot, {
      phase: "recovery_required",
      presentation: "modal",
      transaction: {
        transactionId: "tx-test",
        planId: "plan-test",
        status: "recovery_required",
        checkpoint: "proxy_start_started",
        verification: null,
      },
    });
    try {
      const markup = renderToStaticMarkup(<Onboarding />);
      expect(markup).toContain("Нужно завершить безопасный откат");
      expect(markup).toContain("Повторить безопасный откат");
      expect(markup).not.toContain("Восстановить службу");
      expect(markup).toContain("не означает, что приложение или runtime сломаны");
      expect(markup).toContain("последнего checkpoint");
      expect(containsNestedButton(markup)).toBe(false);
    } finally {
      Object.assign(onboardingMockState.snapshot, {
        phase: "welcome",
        presentation: "required",
        transaction: null,
      });
    }
  });

  it("offers service repair only when readiness confirms it is unavailable", () => {
    Object.assign(onboardingMockState.snapshot, {
      phase: "recovery_required",
      presentation: "modal",
      transaction: {
        transactionId: "tx-test",
        planId: "plan-test",
        status: "recovery_required",
        checkpoint: "rollback_incomplete",
        verification: null,
      },
    });
    Object.assign(onboardingMockState, {
      readiness: {
        service: false,
        dpi: false,
        hosts: false,
        telegram: true,
        protectedResources: true,
        appData: true,
        pendingRecovery: true,
        proxyPort: true,
        repairAvailable: true,
      },
    });
    try {
      const markup = renderToStaticMarkup(<Onboarding />);
      expect(markup).toContain("Восстановить службу");
      expect(markup).not.toContain("Повторить безопасный откат");
    } finally {
      Object.assign(onboardingMockState, { readiness: null });
      Object.assign(onboardingMockState.snapshot, {
        phase: "welcome",
        presentation: "required",
        transaction: null,
      });
    }
  });

  it("renders a deferred rollback as an advisory warning, not a fatal error", () => {
    Object.assign(onboardingMockState, {
      failure: {
        code: "ROLLBACK_FAILED",
        retryable: true,
        messageCode: "onboarding.error.rollback_deferred",
        logPath: null,
      },
    });
    try {
      const markup = renderToStaticMarkup(<Onboarding />);
      expect(markup).toContain("border-warn/25");
      expect(markup).toContain("Сохранённый снимок цел");
      expect(markup).not.toContain("border-danger/25");
    } finally {
      Object.assign(onboardingMockState, { failure: null });
    }
  });

  it("presents a completed rollback as a neutral result without a false network warning", () => {
    Object.assign(onboardingMockState.snapshot, {
      phase: "result",
      presentation: "modal",
      transaction: {
        transactionId: "tx-rolled-back",
        planId: "plan-test",
        status: "rolled_back",
        checkpoint: "rolled_back",
        verification: null,
      },
      verification: null,
      terminalStatus: "completed",
    });
    Object.assign(onboardingMockState, {
      failure: {
        code: "APPLY_FAILED",
        retryable: false,
        messageCode: "onboarding.error.external_change",
        logPath: "C:\\Obsession\\onboarding.log",
      },
    });
    try {
      const markup = renderToStaticMarkup(<Onboarding />);
      expect(markup).toContain("Изменения безопасно отменены");
      expect(markup).toContain("Приложение продолжает работать без изменений");
      expect(markup).toContain("Завершить без изменений");
      expect(markup).toContain('role="status"');
      expect(markup).toContain("border-white/[0.09]");
      expect(markup).not.toContain("Контрольная сеть недоступна");
      expect(markup).not.toContain("border-danger/25");
    } finally {
      Object.assign(onboardingMockState, { failure: null });
      Object.assign(onboardingMockState.snapshot, {
        phase: "welcome",
        presentation: "required",
        transaction: null,
        verification: null,
        terminalStatus: "active",
      });
    }
  });

  it("describes an inconclusive HTTPS check without blaming a control network", () => {
    const verification = {
      outcome: "inconclusive",
      accepted: false,
      targets: [
        {
          id: "dpi",
          label: "Discord / DPI",
          status: "inconclusive",
          message: "Runtime активен; автоматическая HTTPS-проверка Discord не получила ответ",
        },
      ],
    };
    Object.assign(onboardingMockState.snapshot, {
      phase: "result",
      presentation: "modal",
      transaction: {
        transactionId: "tx-inconclusive",
        planId: "plan-test",
        status: "applied",
        checkpoint: "apply_complete",
        verification,
      },
      verification,
      terminalStatus: "active",
    });
    try {
      const markup = renderToStaticMarkup(<Onboarding />);
      expect(markup).toContain("Автопроверка не дала однозначного ответа");
      expect(markup).toContain("адресные HTTPS-проверки не получили ответ");
      expect(markup).toContain("автоматическая HTTPS-проверка Discord");
      expect(markup).not.toContain("Контрольная сеть недоступна");
    } finally {
      Object.assign(onboardingMockState.snapshot, {
        phase: "welcome",
        presentation: "required",
        transaction: null,
        verification: null,
        terminalStatus: "active",
      });
    }
  });
});
