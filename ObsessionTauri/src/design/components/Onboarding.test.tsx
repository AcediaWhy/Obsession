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

  it("renders a named modal, semantic progress and single-control theme tiles", () => {
    const markup = renderToStaticMarkup(<Onboarding />);

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('role="progressbar"');
    expect(markup).toContain('aria-valuenow="1"');
    expect(markup).toContain("bg-base/[0.72]");
    expect(markup).toContain("hover:!bg-accent/90");
    expect(markup).toContain("!text-white");
    expect(markup).toContain("focus-visible:!ring-accent-cyan");
    expect(containsNestedButton(markup)).toBe(false);
  });

  it("uses a dark primary-button foreground on light theme accents", () => {
    expect(getOnboardingPrimaryButtonClass("ophanim")).toContain("!text-[#05060B]");
    expect(getOnboardingPrimaryButtonClass("ophanim")).not.toContain("!text-white");
    expect(getOnboardingPrimaryButtonClass("aurora")).toContain("!text-white");
  });

  it("keeps original core renderers decorative and outside the accessibility tree", () => {
    const markup = renderToStaticMarkup(
      <ThemePreview theme="ophanim" selected size={104} />,
    );

    expect(markup).toContain('aria-hidden="true"');
    expect(markup).not.toContain("<button");
    expect(markup).toContain("<canvas");
    expect(markup).not.toContain("tabindex");
  });
});
