import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("../lib/tauri", () => ({
  api: {},
  runtime: { bootstrap: vi.fn() },
}));

import { useDpiStore } from "./dpiStore";
import type { DpiStatus } from "../lib/tauri";

function status(partial: Partial<DpiStatus>): DpiStatus {
  return { active: false, processes: [], started_at: null, ...partial };
}

describe("dpiStore.applyStatus", () => {
  beforeEach(() => {
    useDpiStore.setState({
      revision: -1,
      active: false,
      processes: [],
      startedAt: null,
      transitioning: false,
    });
  });

  it("конвертирует started_at из Unix-сек в мс", () => {
    useDpiStore.getState().applyStatus(status({ active: true, started_at: 1_700_000 }));
    expect(useDpiStore.getState().startedAt).toBe(1_700_000_000);
    expect(useDpiStore.getState().active).toBe(true);
  });

  it("started_at=null → startedAt=null (обход выключен)", () => {
    useDpiStore.setState({ startedAt: 123456 });
    useDpiStore.getState().applyStatus(status({ active: false, started_at: null }));
    expect(useDpiStore.getState().startedAt).toBeNull();
  });

  it("НЕ трогает transitioning — им владеют start/stop", () => {
    useDpiStore.setState({ transitioning: true });
    useDpiStore.getState().applyStatus(status({ active: true, started_at: 1 }));
    expect(useDpiStore.getState().transitioning).toBe(true);
  });

  it("не даёт старому snapshot перетереть более свежее событие", () => {
    expect(
      useDpiStore.getState().applyVersionedStatus({
        revision: 2,
        value: status({ active: true, started_at: 10 }),
      }),
    ).toBe(true);
    expect(
      useDpiStore.getState().applyVersionedStatus({
        revision: 1,
        value: status({ active: false, started_at: null }),
      }),
    ).toBe(false);
    expect(useDpiStore.getState().revision).toBe(2);
    expect(useDpiStore.getState().active).toBe(true);
    expect(useDpiStore.getState().startedAt).toBe(10_000);
  });
});
