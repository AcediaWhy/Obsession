import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/tauri", () => ({
  on: { log: vi.fn() },
}));

import { useLogStore } from "./logStore";
import type { LogEvent } from "../lib/tauri";

function event(index: number): LogEvent {
  return {
    level: "info",
    source: "test",
    message: "line " + index,
    ts: String(index),
  };
}

describe("logStore frame batching", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useLogStore.getState().clear();
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("commits a burst once and assigns stable monotonic ids", () => {
    useLogStore.getState().push(event(1));
    useLogStore.getState().push(event(2));
    useLogStore.getState().push(event(3));

    expect(useLogStore.getState().lines).toEqual([]);

    vi.runOnlyPendingTimers();

    const lines = useLogStore.getState().lines;
    expect(lines.map((line) => line.message)).toEqual(["line 1", "line 2", "line 3"]);
    expect(new Set(lines.map((line) => line.id)).size).toBe(3);
    expect(lines[1].id).toBe(lines[0].id + 1);
  });

  it("keeps only the latest 200 lines", () => {
    for (let index = 0; index < 205; index += 1) {
      useLogStore.getState().push(event(index));
    }

    vi.runOnlyPendingTimers();

    const lines = useLogStore.getState().lines;
    expect(lines).toHaveLength(200);
    expect(lines[0].message).toBe("line 5");
    expect(lines[199].message).toBe("line 204");
  });

  it("clear drops both committed and not-yet-flushed lines", () => {
    useLogStore.getState().push(event(1));
    useLogStore.getState().clear();

    vi.runOnlyPendingTimers();

    expect(useLogStore.getState().lines).toEqual([]);
  });
});
