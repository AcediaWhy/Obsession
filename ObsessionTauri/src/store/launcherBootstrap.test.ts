import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/tauri", () => ({
  api: {},
  on: {},
  runtime: {},
}));

import {
  createLauncherBootstrap,
  type LauncherBootstrapPorts,
} from "./launcherBootstrap";
import type {
  AdaptiveStatus,
  AppConfig,
  BootstrapSnapshot,
  BrainStatus,
  DpiStatus,
  LegacyReliabilityStatus,
  ProxyStatus,
  Settings,
  VersionedSection,
} from "../lib/tauri";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const settings: Settings = {
  minimize_to_tray: true,
  start_minimized: false,
  selected_categories: ["discord"],
  zapret2_selected_categories: ["discord"],
  selected_configs: {},
  proxy_port: 1443,
  fake_tls_domain: "",
  ai_provider: "malw",
  has_completed_onboarding: true,
  auto_recovery: false,
  reduce_motion: false,
  hotkey_toggle: "Ctrl+Shift+KeyO",
  lan_publish_secs: 0,
  dpi_engine: "zapret2",
  zapret2_level: 0,
  adaptive_strategy_enabled: true,
  adaptive_search_mode: "balanced",
};

const config: AppConfig = {
  categories: ["discord"],
  configs: { discord: ["discord_1.conf"] },
  lists: [],
};

function legacyStatus(
  phase: LegacyReliabilityStatus["phase"],
): LegacyReliabilityStatus {
  return {
    mode: "observe_only",
    phase,
    activeCategories: phase === "inactive" ? [] : ["discord"],
    sessionId: phase === "inactive" ? null : 11,
    sensorGeneration: phase === "observing" ? 3 : null,
  };
}

function makeSnapshot(revisions: Partial<Record<string, number>> = {}) {
  return {
    schemaVersion: 2,
    settings: {
      revision: revisions.settings ?? 1,
      value: { settings, elevated: true, autostart: false },
    },
    dpi: {
      revision: revisions.dpi ?? 1,
      value: {
        active: false,
        processes: [],
        started_at: null,
      } as DpiStatus,
    },
    proxy: {
      revision: revisions.proxy ?? 1,
      value: {
        running: false,
        link: "",
        lan_link: null,
        lan_published: false,
        lan_expiry_unix: null,
      },
    },
    brain: {
      revision: revisions.brain ?? 1,
      value: null,
    },
    adaptive: {
      revision: revisions.adaptive ?? 1,
      value: { phase: "idle" } as AdaptiveStatus,
    },
    legacyReliability: {
      revision: revisions.legacyReliability ?? 1,
      value: legacyStatus("inactive"),
    },
    hosts: {
      revision: revisions.hosts ?? 1,
      value: {
        provider: "malw",
        status: "not_installed",
        local_version: "",
        remote_version: "",
        rollback_available: false,
      },
    },
  } satisfies BootstrapSnapshot;
}

type SectionKey =
  | "settings"
  | "dpi"
  | "proxy"
  | "brain"
  | "adaptive"
  | "legacyReliability"
  | "hosts";

function createHarness(listenerGate?: Promise<void>) {
  const revisions: Record<SectionKey, number> = {
    settings: -1,
    dpi: -1,
    proxy: -1,
    brain: -1,
    adaptive: -1,
    legacyReliability: -1,
    hosts: -1,
  };
  const values: Partial<Record<SectionKey, unknown>> = {};
  const callbacks: {
    dpi?: (section: VersionedSection<DpiStatus>) => void;
    proxy?: (section: VersionedSection<ProxyStatus>) => void;
    brain?: (section: VersionedSection<BrainStatus>) => void;
    adaptive?: (section: VersionedSection<AdaptiveStatus>) => void;
    legacyReliability?: (
      section: VersionedSection<LegacyReliabilityStatus>,
    ) => void;
  } = {};
  const unlisteners = Array.from({ length: 8 }, () => vi.fn());
  let unlistenIndex = 0;
  const snapshots: Array<Promise<BootstrapSnapshot>> = [];

  const apply = <T>(
    key: SectionKey,
    section: VersionedSection<T>,
  ): boolean => {
    if (section.revision <= revisions[key]) return false;
    revisions[key] = section.revision;
    values[key] = section.value;
    return true;
  };

  const register = async <T>(
    key: keyof typeof callbacks,
    cb: (section: VersionedSection<T>) => void,
  ) => {
    callbacks[key] = cb as never;
    if (listenerGate) await listenerGate;
    return unlisteners[unlistenIndex++];
  };

  const ports: LauncherBootstrapPorts = {
    listenDpi: vi.fn((cb) => register("dpi", cb)),
    listenProxy: vi.fn((cb) => register("proxy", cb)),
    listenBrain: vi.fn((cb) => register("brain", cb)),
    listenAdaptive: vi.fn((cb) => register("adaptive", cb)),
    listenLegacyReliability: vi.fn((cb) =>
      register("legacyReliability", cb),
    ),
    listenSuggestion: vi.fn(async () => {
      if (listenerGate) await listenerGate;
      return unlisteners[unlistenIndex++];
    }),
    listenProbe: vi.fn(async () => {
      if (listenerGate) await listenerGate;
      return unlisteners[unlistenIndex++];
    }),
    subscribeSettings: () => unlisteners[unlistenIndex++],
    getSnapshot: vi.fn(() => {
      const next = snapshots.shift();
      if (!next) throw new Error("snapshot queue is empty");
      return next;
    }),
    getConfig: vi.fn(async () => config),
    getProxyAvailable: vi.fn(async () => true),
    applySettings: (section) => apply("settings", section),
    applyDpi: (section) => apply("dpi", section),
    applyProxy: (section) => apply("proxy", section),
    applyBrain: (section) => apply("brain", section),
    applyAdaptive: (section) => apply("adaptive", section),
    applyLegacyReliability: (section) =>
      apply("legacyReliability", section),
    applyHosts: (section) => apply("hosts", section),
    applySuggestion: vi.fn(),
    applyProbe: vi.fn(),
    initializeDpi: vi.fn(),
    initializeProxy: vi.fn(),
    loadDpiMetadata: vi.fn(async () => {}),
    afterDpiEvent: vi.fn(),
    reportError: vi.fn(),
  };

  return {
    ports,
    callbacks,
    revisions,
    values,
    unlisteners,
    enqueueSnapshot: (snapshot: Promise<BootstrapSnapshot>) =>
      snapshots.push(snapshot),
  };
}

describe("launcherBootstrap", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("registers listeners before snapshot and rejects stale overlapping sections", async () => {
    const listenersReady = deferred<void>();
    const snapshotReady = deferred<BootstrapSnapshot>();
    const harness = createHarness(listenersReady.promise);
    harness.enqueueSnapshot(snapshotReady.promise);
    const bootstrap = createLauncherBootstrap(harness.ports);

    const release = bootstrap.acquire();
    await Promise.resolve();
    expect(harness.ports.getSnapshot).not.toHaveBeenCalled();

    listenersReady.resolve();
    await vi.waitFor(() =>
      expect(harness.ports.getSnapshot).toHaveBeenCalledTimes(1),
    );

    harness.callbacks.dpi?.({
      revision: 2,
      value: {
        active: true,
        processes: [],
        started_at: 10,
      } as DpiStatus,
    });
    harness.callbacks.legacyReliability?.({
      revision: 2,
      value: legacyStatus("observing"),
    });
    snapshotReady.resolve(makeSnapshot({ dpi: 1, legacyReliability: 1 }));
    await bootstrap.whenReady();

    expect(harness.revisions.dpi).toBe(2);
    expect((harness.values.dpi as DpiStatus).active).toBe(true);
    expect(harness.revisions.legacyReliability).toBe(2);
    expect(
      (harness.values.legacyReliability as LegacyReliabilityStatus).phase,
    ).toBe("observing");
    release();
  });

  it("keeps listeners alive when the initial snapshot fails", async () => {
    const harness = createHarness();
    harness.enqueueSnapshot(Promise.reject(new Error("snapshot failed")));
    const bootstrap = createLauncherBootstrap(harness.ports);

    const release = bootstrap.acquire();
    await bootstrap.whenReady();
    expect(harness.ports.reportError).toHaveBeenCalledWith(
      "snapshot",
      expect.any(Error),
    );

    harness.callbacks.proxy?.({
      revision: 3,
      value: {
        running: true,
        link: "tg://proxy",
        lan_link: null,
        lan_published: false,
        lan_expiry_unix: null,
      },
    });
    expect(harness.revisions.proxy).toBe(3);
    release();
  });

  it("reuses one listener session across a StrictMode cleanup-remount", async () => {
    const harness = createHarness();
    harness.enqueueSnapshot(Promise.resolve(makeSnapshot()));
    const bootstrap = createLauncherBootstrap(harness.ports);

    const releaseFirst = bootstrap.acquire();
    releaseFirst();
    const releaseSecond = bootstrap.acquire();
    await bootstrap.whenReady();

    expect(harness.ports.listenDpi).toHaveBeenCalledTimes(1);
    expect(harness.ports.listenProxy).toHaveBeenCalledTimes(1);
    expect(harness.ports.listenLegacyReliability).toHaveBeenCalledTimes(1);

    releaseSecond();
    await Promise.resolve();
    await Promise.resolve();
    expect(harness.unlisteners.every((unlisten) => unlisten.mock.calls.length === 1))
      .toBe(true);
  });

  it("applies unrelated resume sections independently by revision", async () => {
    const harness = createHarness();
    harness.enqueueSnapshot(Promise.resolve(makeSnapshot()));
    const bootstrap = createLauncherBootstrap(harness.ports);
    const release = bootstrap.acquire();
    await bootstrap.whenReady();

    const resume = deferred<BootstrapSnapshot>();
    harness.enqueueSnapshot(resume.promise);
    const refreshing = bootstrap.refresh();
    await Promise.resolve();

    harness.callbacks.adaptive?.({
      revision: 3,
      value: { phase: "searching" } as AdaptiveStatus,
    });
    resume.resolve(makeSnapshot({ dpi: 2, adaptive: 2 }));
    await refreshing;

    expect(harness.revisions.dpi).toBe(2);
    expect(harness.revisions.adaptive).toBe(3);
    expect((harness.values.adaptive as AdaptiveStatus).phase).toBe(
      "searching",
    );
    release();
  });

  it("rejects an unsupported bootstrap schema through the snapshot error path", async () => {
    const harness = createHarness();
    harness.enqueueSnapshot(
      Promise.resolve({ ...makeSnapshot(), schemaVersion: 1 }),
    );
    const bootstrap = createLauncherBootstrap(harness.ports);

    const release = bootstrap.acquire();
    await bootstrap.whenReady();

    expect(harness.ports.reportError).toHaveBeenCalledWith(
      "snapshot",
      expect.objectContaining({
        message: "Unsupported bootstrap schema 1",
      }),
    );
    expect(harness.revisions.legacyReliability).toBe(-1);
    release();
  });

  it("does not refresh without a complete listener session", async () => {
    const harness = createHarness();
    harness.ports.listenDpi = vi.fn(async () => {
      throw new Error("listen failed");
    });
    const bootstrap = createLauncherBootstrap(harness.ports);

    const release = bootstrap.acquire();
    await bootstrap.whenReady();
    expect(harness.ports.getSnapshot).not.toHaveBeenCalled();

    await bootstrap.refresh();
    expect(harness.ports.getSnapshot).not.toHaveBeenCalled();
    expect(harness.ports.reportError).toHaveBeenCalledWith(
      "listeners",
      expect.any(Error),
    );
    release();
  });
});
