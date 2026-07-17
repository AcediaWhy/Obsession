import { describe, expect, it } from "vitest";

import { beginMountedCycle } from "./diagnosticsLifecycle";

describe("diagnostics lifecycle", () => {
  it("reactivates updates after a StrictMode cleanup-remount cycle", () => {
    const mounted = { current: false };

    const cleanupFirst = beginMountedCycle(mounted);
    expect(mounted.current).toBe(true);

    cleanupFirst();
    expect(mounted.current).toBe(false);

    const cleanupSecond = beginMountedCycle(mounted);
    expect(mounted.current).toBe(true);

    cleanupSecond();
    expect(mounted.current).toBe(false);
  });
});
