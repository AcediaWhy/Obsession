import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  dpiStart: vi.fn(),
  dpiStop: vi.fn(),
  proxyStart: vi.fn(),
  proxyStop: vi.fn(),
  bootstrap: vi.fn(),
}));

vi.mock("../lib/tauri", () => ({
  api: {
    updateSettings: mocks.updateSettings,
    dpiStart: mocks.dpiStart,
    dpiStop: mocks.dpiStop,
    proxyStart: mocks.proxyStart,
    proxyStop: mocks.proxyStop,
  },
  runtime: { bootstrap: mocks.bootstrap },
  clipboard: { write: vi.fn() },
}));

import {
  TRANSITION_RECONCILE_TIMEOUT_MS,
  TRANSITION_WATCHDOG_MS,
  useDpiStore,
} from "./dpiStore";
import {
  PROXY_TRANSITION_TIMEOUT_MS,
  useProxyStore,
} from "./proxyStore";

const never = () => new Promise<never>(() => {});

async function flushMicrotasks() {
  for (let index = 0; index < 6; index += 1) await Promise.resolve();
}

describe("transition watchdogs", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.updateSettings.mockResolvedValue(undefined);
    useDpiStore.setState({
      revision: -1,
      active: false,
      processes: [],
      startedAt: null,
      transitioning: false,
      error: "",
    });
    useProxyStore.setState({
      revision: -1,
      running: false,
      link: "",
      lanLink: null,
      transitioning: false,
      error: "",
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("releases DPI controls and preserves a late live status", async () => {
    mocks.dpiStart.mockImplementation(never);
    const operation = useDpiStore.getState().start();
    await flushMicrotasks();

    expect(mocks.dpiStart).toHaveBeenCalledOnce();
    useDpiStore.getState().applyStatus({
      active: true,
      processes: [{ category: "discord", config_file: "discord_1.conf", pid: 42 }],
      started_at: 1_700_000_000,
    });

    await vi.advanceTimersByTimeAsync(TRANSITION_WATCHDOG_MS);
    await operation;

    expect(useDpiStore.getState()).toMatchObject({
      active: true,
      transitioning: false,
    });
    expect(useDpiStore.getState().error).toContain("не ответил вовремя");
  });

  it("releases proxy controls and preserves a late live status", async () => {
    mocks.proxyStart.mockImplementation(never);
    const operation = useProxyStore.getState().start();

    useProxyStore.getState().applyStatus({
      running: true,
      link: "tg://proxy?server=127.0.0.1&port=1443&secret=dd00",
      lan_link: null,
      lan_published: false,
      lan_expiry_unix: null,
    });

    await vi.advanceTimersByTimeAsync(PROXY_TRANSITION_TIMEOUT_MS);
    await operation;

    expect(useProxyStore.getState()).toMatchObject({
      running: true,
      transitioning: false,
    });
    expect(useProxyStore.getState().error).toContain("не ответил вовремя");
  });

  it("releases a DPI latch when even runtime reconciliation never answers", async () => {
    mocks.bootstrap.mockImplementation(never);
    useDpiStore.setState({ transitioning: true });
    const reconciliation = useDpiStore.getState().reconcileTransition();

    await vi.advanceTimersByTimeAsync(TRANSITION_RECONCILE_TIMEOUT_MS);
    await reconciliation;

    expect(useDpiStore.getState().transitioning).toBe(false);
    expect(useDpiStore.getState().error).toContain("не ответил");
  });
});
