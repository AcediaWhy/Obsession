import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  api,
  on,
  runtime,
  type AdaptiveProbeBatch,
  type AdaptiveStatus,
  type AdaptiveSuggestion,
  type AppConfig,
  type BootstrapSnapshot,
  type BrainStatus,
  type DpiStatus,
  type ProxyStatus,
  type Settings,
  type VersionedSection,
} from "../lib/tauri";
import { useAdaptiveStrategyStore } from "./adaptiveStrategyStore";
import { useBrainStore } from "./brainStore";
import { useDpiStore } from "./dpiStore";
import { subscribeHostsToSettings, useHostsStore } from "./hostsStore";
import { useProxyStore } from "./proxyStore";
import { useSettingsStore } from "./settingsStore";

const BOOTSTRAP_SCHEMA_VERSION = 1;

type BootstrapErrorScope = "listeners" | "snapshot" | "dpi" | "proxy";

export interface LauncherBootstrapPorts {
  listenDpi: (
    cb: (section: VersionedSection<DpiStatus>) => void,
  ) => Promise<UnlistenFn>;
  listenProxy: (
    cb: (section: VersionedSection<ProxyStatus>) => void,
  ) => Promise<UnlistenFn>;
  listenBrain: (
    cb: (section: VersionedSection<BrainStatus>) => void,
  ) => Promise<UnlistenFn>;
  listenAdaptive: (
    cb: (section: VersionedSection<AdaptiveStatus>) => void,
  ) => Promise<UnlistenFn>;
  listenSuggestion: (
    cb: (suggestion: AdaptiveSuggestion) => void,
  ) => Promise<UnlistenFn>;
  listenProbe: (
    cb: (probe: AdaptiveProbeBatch) => void,
  ) => Promise<UnlistenFn>;
  subscribeSettings: () => UnlistenFn;
  getSnapshot: () => Promise<BootstrapSnapshot>;
  getConfig: () => Promise<AppConfig>;
  getProxyAvailable: () => Promise<boolean>;
  applySettings: (
    section: BootstrapSnapshot["settings"],
  ) => boolean;
  applyDpi: (section: BootstrapSnapshot["dpi"]) => boolean;
  applyProxy: (section: BootstrapSnapshot["proxy"]) => boolean;
  applyBrain: (section: BootstrapSnapshot["brain"]) => boolean;
  applyAdaptive: (
    section: BootstrapSnapshot["adaptive"],
  ) => boolean;
  applyHosts: (section: BootstrapSnapshot["hosts"]) => boolean;
  applySuggestion: (suggestion: AdaptiveSuggestion) => void;
  applyProbe: (probe: AdaptiveProbeBatch) => void;
  initializeDpi: (config: AppConfig, settings: Settings) => void;
  initializeProxy: (settings: Settings, available: boolean) => void;
  loadDpiMetadata: () => Promise<void>;
  afterDpiEvent: () => void;
  reportError: (scope: BootstrapErrorScope, error: unknown) => void;
}

function cleanupAll(unlisteners: UnlistenFn[]): void {
  for (const unlisten of unlisteners.splice(0, unlisteners.length)) {
    try {
      unlisten();
    } catch {
      // Cleanup is best-effort; another listener must still be released.
    }
  }
}

function applySnapshot(
  ports: LauncherBootstrapPorts,
  snapshot: BootstrapSnapshot,
): void {
  if (snapshot.schemaVersion !== BOOTSTRAP_SCHEMA_VERSION) {
    throw new Error(
      `Unsupported bootstrap schema ${snapshot.schemaVersion}`,
    );
  }

  // Each reducer owns its revision. An adaptive event cannot suppress a DPI
  // section from the same snapshot, and one bad/stale section does not gate the rest.
  ports.applyHosts(snapshot.hosts);
  ports.applySettings(snapshot.settings);
  ports.applyDpi(snapshot.dpi);
  ports.applyProxy(snapshot.proxy);
  ports.applyBrain(snapshot.brain);
  ports.applyAdaptive(snapshot.adaptive);
}

async function hydrateStartup(ports: LauncherBootstrapPorts): Promise<void> {
  const [snapshotResult, configResult, proxyResult] =
    await Promise.allSettled([
      ports.getSnapshot(),
      ports.getConfig(),
      ports.getProxyAvailable(),
    ]);

  let snapshot: BootstrapSnapshot | null = null;
  if (snapshotResult.status === "fulfilled") {
    try {
      applySnapshot(ports, snapshotResult.value);
      snapshot = snapshotResult.value;
    } catch (error) {
      ports.reportError("snapshot", error);
    }
  } else {
    ports.reportError("snapshot", snapshotResult.reason);
  }

  if (snapshot && configResult.status === "fulfilled") {
    ports.initializeDpi(
      configResult.value,
      snapshot.settings.value.settings,
    );
  } else if (configResult.status === "rejected") {
    ports.reportError("dpi", configResult.reason);
  }

  if (snapshot && proxyResult.status === "fulfilled") {
    ports.initializeProxy(
      snapshot.settings.value.settings,
      proxyResult.value,
    );
  } else if (proxyResult.status === "rejected") {
    ports.reportError("proxy", proxyResult.reason);
  }

  try {
    await ports.loadDpiMetadata();
  } catch (error) {
    ports.reportError("dpi", error);
  }
}

async function startSession(
  ports: LauncherBootstrapPorts,
): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = [ports.subscribeSettings()];
  const registrations = await Promise.allSettled([
    ports.listenDpi((section) => {
      if (ports.applyDpi(section)) ports.afterDpiEvent();
    }),
    ports.listenProxy(ports.applyProxy),
    ports.listenBrain((section) => ports.applyBrain(section)),
    ports.listenAdaptive((section) => ports.applyAdaptive(section)),
    ports.listenSuggestion(ports.applySuggestion),
    ports.listenProbe(ports.applyProbe),
  ]);

  const failed = registrations.find(
    (result): result is PromiseRejectedResult =>
      result.status === "rejected",
  );
  for (const result of registrations) {
    if (result.status === "fulfilled") unlisteners.push(result.value);
  }
  if (failed) {
    cleanupAll(unlisteners);
    throw failed.reason;
  }

  // Snapshot invoke starts only after every listener registration has resolved.
  await hydrateStartup(ports);
  return () => cleanupAll(unlisteners);
}

interface BootstrapSession {
  leases: number;
  ready: Promise<UnlistenFn | null>;
}

export function createLauncherBootstrap(ports: LauncherBootstrapPorts) {
  let session: BootstrapSession | null = null;

  const acquire = (): UnlistenFn => {
    if (!session) {
      session = {
        leases: 0,
        ready: startSession(ports).catch((error) => {
          ports.reportError("listeners", error);
          return null;
        }),
      };
    }

    const current = session;
    current.leases += 1;
    let released = false;

    return () => {
      if (released) return;
      released = true;
      current.leases = Math.max(0, current.leases - 1);
      queueMicrotask(() => {
        if (session !== current || current.leases !== 0) return;
        session = null;
        void current.ready.then((cleanup) => cleanup?.());
      });
    };
  };

  const refresh = async (): Promise<void> => {
    const current = session;
    if (!current) return;
    const cleanup = await current.ready;
    if (session !== current || !cleanup) return;
    try {
      applySnapshot(ports, await ports.getSnapshot());
    } catch (error) {
      ports.reportError("snapshot", error);
    }
  };

  const whenReady = (): Promise<void> =>
    session?.ready.then(() => undefined) ?? Promise.resolve();

  return { acquire, refresh, whenReady };
}

const realPorts: LauncherBootstrapPorts = {
  listenDpi: on.dpiStatusVersioned,
  listenProxy: on.proxyStatusVersioned,
  listenBrain: on.brainStatusVersioned,
  listenAdaptive: on.adaptiveStatusVersioned,
  listenSuggestion: on.adaptiveSuggestion,
  listenProbe: on.adaptiveProbe,
  subscribeSettings: subscribeHostsToSettings,
  getSnapshot: runtime.bootstrap,
  getConfig: api.getConfig,
  getProxyAvailable: api.proxyAvailable,
  applySettings: (section) =>
    useSettingsStore.getState().applyBootstrap(section),
  applyDpi: (section) =>
    useDpiStore.getState().applyVersionedStatus(section),
  applyProxy: (section) =>
    useProxyStore.getState().applyVersionedStatus(section),
  applyBrain: (section) =>
    useBrainStore.getState().applyVersionedStatus(section),
  applyAdaptive: (section) =>
    useAdaptiveStrategyStore.getState().applyVersionedStatus(section),
  applyHosts: (section) =>
    useHostsStore.getState().applyVersionedStatus(section),
  applySuggestion: (suggestion) =>
    useAdaptiveStrategyStore.setState({ suggestion }),
  applyProbe: (probe) => useAdaptiveStrategyStore.setState({ probe }),
  initializeDpi: (config, settings) =>
    useDpiStore.getState().initialize(config, settings),
  initializeProxy: (settings, available) =>
    useProxyStore.getState().initialize(settings, available),
  loadDpiMetadata: async () => {
    const dpi = useDpiStore.getState();
    await Promise.all([
      dpi.loadStats(),
      dpi.loadEngines(),
      dpi.loadZapret2Profiles(),
    ]);
  },
  afterDpiEvent: () => {
    const dpi = useDpiStore.getState();
    void Promise.all([dpi.loadEngines(), dpi.loadZapret2Profiles()]);
  },
  reportError: (scope, error) => {
    const message = String(error);
    if (scope === "snapshot" || scope === "listeners") {
      useSettingsStore.getState().failBootstrap(message);
    }
    if (scope === "dpi" || scope === "listeners") {
      useDpiStore.setState({ error: message });
    }
    if (scope === "proxy" || scope === "listeners") {
      useProxyStore.setState({ error: message });
    }
  },
};

export const launcherBootstrap = createLauncherBootstrap(realPorts);
