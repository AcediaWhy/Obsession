import { describe, expect, it } from "vitest";
import { helixStage } from "./state";
import type { Zapret2PickerProgress } from "../../../lib/tauri";

const progress: Zapret2PickerProgress = { category: "discord", transport: "tls", current: 1,
  total: 5, profileId: "candidate", label: "Candidate", phase: "screening" };

describe("picker choreography follows verified state", () => {
  it("never celebrates while a candidate is being checked or restored", () => {
    expect(helixStage(progress, false, false, true, false)).toBe("searching");
    expect(helixStage({ ...progress, phase: "confirmation" }, false, false, true, false)).toBe("confirming");
    expect(helixStage({ ...progress, phase: "restoring" }, false, false, true, false)).toBe("restoring");
  });
  it("gives cancellation priority over confirmations and successful results", () => {
    expect(helixStage({ ...progress, phase: "confirmation" }, true, false, true, false)).toBe("cancelling");
    expect(helixStage(progress, false, true, true, true)).toBe("cancelled");
    expect(helixStage(progress, true, true, true, false)).toBe("cancelled");
  });
  it("only shows the success outro after a confirmed result and completion", () => {
    expect(helixStage(progress, false, true, false, false)).toBe("finished");
    expect(helixStage(progress, false, true, true, false)).toBe("success");
  });
});
