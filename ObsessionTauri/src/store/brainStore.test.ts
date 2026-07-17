import { beforeEach, describe, expect, it } from "vitest";

import { useBrainStore } from "./brainStore";
import type { BrainStatus } from "../lib/tauri";

const status = { enabled: true, phase: "idle" } as BrainStatus;

describe("brainStore", () => {
  beforeEach(() => {
    useBrainStore.setState({ revision: -1, status: null });
  });

  it("keeps a newer event and accepts a later null snapshot", () => {
    expect(
      useBrainStore.getState().applyVersionedStatus({
        revision: 2,
        value: status,
      }),
    ).toBe(true);
    expect(
      useBrainStore.getState().applyVersionedStatus({
        revision: 1,
        value: null,
      }),
    ).toBe(false);
    expect(useBrainStore.getState().status).toBe(status);

    expect(
      useBrainStore.getState().applyVersionedStatus({
        revision: 3,
        value: null,
      }),
    ).toBe(true);
    expect(useBrainStore.getState().status).toBeNull();
  });
});
