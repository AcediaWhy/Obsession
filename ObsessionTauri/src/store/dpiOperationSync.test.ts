import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  dpiStart: vi.fn(), dpiStop: vi.fn(), dpiEngineSet: vi.fn(),
  updateSettings: vi.fn(), bootstrap: vi.fn(), dpiTest: vi.fn(),
}));
vi.mock("../lib/tauri", () => ({
  api: mocks, runtime: { bootstrap: mocks.bootstrap },
}));
import { useDpiStore } from "./dpiStore";

function snapshot(active: boolean, revision = 1) {
  return { dpi: { revision, value: { active, processes: [], started_at: active ? 123 : null } } };
}

describe("DPI operation reconciliation", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useDpiStore.setState({ active: false, transitioning: false, testing: false, revision: 1, error: "", engines: [], selectedCategories: ["discord"], selectedConfigs: { discord: "discord_1.conf" } });
  });

  it("restores the Stop button after the service rejects an engine change as active", async () => {
    mocks.dpiEngineSet.mockRejectedValue(new Error("Сначала выключите защиту"));
    mocks.bootstrap.mockResolvedValue(snapshot(true));
    await useDpiStore.getState().setEngine("zapret2");
    expect(useDpiStore.getState()).toMatchObject({ active: true, transitioning: false, engines: [] });
    expect(useDpiStore.getState().error).toContain("выключите");
  });

  it("recovers a lost start event from a same-revision service snapshot", async () => {
    mocks.bootstrap.mockResolvedValue(snapshot(true));
    await useDpiStore.getState().start();
    expect(useDpiStore.getState()).toMatchObject({ active: true, transitioning: false });
  });

  it("reveals a test session left active after cleanup failed", async () => {
    mocks.dpiTest.mockRejectedValue(new Error("cleanup failed"));
    mocks.bootstrap.mockResolvedValue(snapshot(true));
    await useDpiStore.getState().testAll();
    expect(useDpiStore.getState()).toMatchObject({ active: true, testing: false });
    expect(useDpiStore.getState().error).toContain("cleanup failed");
  });

  it("reflects a completed stop even if its response failed", async () => {
    useDpiStore.setState({ active: true });
    mocks.dpiStop.mockRejectedValue(new Error("reply lost"));
    mocks.bootstrap.mockResolvedValue(snapshot(false));
    await useDpiStore.getState().stop();
    expect(useDpiStore.getState()).toMatchObject({ active: false, transitioning: false });
  });

  it("does not overwrite a newer event with an older snapshot", async () => {
    useDpiStore.setState({ active: true, revision: 5 });
    mocks.bootstrap.mockResolvedValue(snapshot(false, 4));
    await useDpiStore.getState().stop();
    expect(useDpiStore.getState().active).toBe(true);
  });

  it("preserves the last status and operation error when the service is unreachable", async () => {
    useDpiStore.setState({ active: true });
    mocks.dpiStop.mockRejectedValue(new Error("stop failed"));
    mocks.bootstrap.mockRejectedValue(new Error("offline"));
    await useDpiStore.getState().stop();
    expect(useDpiStore.getState()).toMatchObject({ active: true, transitioning: false });
    expect(useDpiStore.getState().error).toContain("stop failed");
  });
});
