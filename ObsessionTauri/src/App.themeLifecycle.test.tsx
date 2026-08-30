import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

const motionState = vi.hoisted(() => ({ isPresent: true }));

vi.mock("framer-motion", () => ({
  AnimatePresence: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
  MotionConfig: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
  motion: {
    div: ({ children, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
      <div {...props}>{children}</div>
    ),
  },
  useIsPresent: () => motionState.isPresent,
}));

vi.mock("./design/components/HeroField", () => ({
  HeroField: (props: { theme: string; frozen: boolean }) => (
    <div
      data-heavy-hero-field
      data-theme={props.theme}
      data-frozen={String(props.frozen)}
    />
  ),
}));

vi.mock("./design/parallax", () => ({
  ParallaxProvider: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
  Parallax: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
}));
vi.mock("./design/components/CustomTitleBar", () => ({ CustomTitleBar: () => null }));
vi.mock("./design/components/NavRail", () => ({
  NavRail: () => null,
  TAB_ORDER: [],
}));
vi.mock("./screens/Overview", () => ({ OverviewScreen: () => null }));
vi.mock("./screens/Dpi", () => ({ DpiScreen: () => null }));
vi.mock("./screens/Ai", () => ({ AiScreen: () => null }));
vi.mock("./screens/Telegram", () => ({ TelegramScreen: () => null }));
vi.mock("./screens/Lists", () => ({ ListsScreen: () => null }));
vi.mock("./screens/Settings", () => ({ SettingsScreen: () => null }));
vi.mock("./screens/Profiles", () => ({ ProfilesScreen: () => null }));
vi.mock("./store/logStore", () => ({ initLogStream: () => () => {} }));
vi.mock("./store/launcherBootstrap", () => ({
  launcherBootstrap: { acquire: () => () => {}, refresh: async () => {}, whenReady: async () => {} },
}));
vi.mock("./store/themeStore", () => ({
  useThemeStore: () => "aurora",
}));
vi.mock("./store/settingsStore", () => ({
  useSettingsStore: () => undefined,
}));
vi.mock("./store/onboardingStore", () => ({
  useOnboardingStore: () => undefined,
}));
vi.mock("./design/components/Onboarding", () => ({ Onboarding: () => null }));
vi.mock("./design/components/Toaster", () => ({ Toaster: () => null }));
vi.mock("./lib/tauri", () => ({ on: { windowVisibility: () => Promise.resolve(() => {}), log: () => Promise.resolve(() => {}) } }));
vi.mock("./design/render", () => ({
  setWindowShown: () => {},
  useMotionOff: () => false,
  useRenderHidden: () => false,
}));
vi.mock("./design/gl/trayStageRelease", () => ({
  initTrayStageRelease: () => () => {},
}));
vi.mock("./design/screenTransition", () => ({ screenVariants: {} }));
vi.mock("./design/tokens", () => ({
  dur: { fast: 0.14, slow: 0.6 },
  ease: { xfade: "easeInOut" },
  spring: { rise: {} },
}));
vi.mock("./store/toastStore", () => ({ toast: { error: () => {} } }));
vi.mock("./design/useObsessionVisualPhase", () => ({ useObsessionVisualPhase: () => "idle" }));
vi.mock("./design/obsessionVisualState", () => ({ obsessionFocusForScreen: () => ({ x: 0, y: 0 }) }));

describe("ThemeScene resource lifecycle", () => {
  it("unmounts the heavy HeroField immediately while an exiting shell remains", async () => {
    const { default: App } = await import("./App");
    motionState.isPresent = false;

    const markup = renderToStaticMarkup(
      <App />,
    );

    expect(markup).toContain("data-theme-scene-exit-placeholder");
    expect(markup).not.toContain("data-heavy-hero-field");
  });

  it("renders the heavy HeroField for the present scene", async () => {
    const { default: App } = await import("./App");
    motionState.isPresent = true;

    const markup = renderToStaticMarkup(
      <App />,
    );

    expect(markup).toContain("data-heavy-hero-field");
    expect(markup).not.toContain("data-theme-scene-exit-placeholder");
  });
});
