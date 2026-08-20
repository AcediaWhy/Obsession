import { afterEach, describe, expect, it, vi } from "vitest";

import { OperationTimeoutError, withDeadline } from "./asyncDeadline";

describe("withDeadline", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns a result that arrives before the deadline", async () => {
    await expect(withDeadline(Promise.resolve("ok"), 100, "late")).resolves.toBe("ok");
  });

  it("releases the caller when an operation never settles", async () => {
    vi.useFakeTimers();
    const pending = withDeadline(new Promise<never>(() => {}), 8_000, "timed out");
    const assertion = expect(pending).rejects.toEqual(
      expect.objectContaining<Partial<OperationTimeoutError>>({
        name: "OperationTimeoutError",
        message: "timed out",
      }),
    );

    await vi.advanceTimersByTimeAsync(8_000);
    await assertion;
  });
});
