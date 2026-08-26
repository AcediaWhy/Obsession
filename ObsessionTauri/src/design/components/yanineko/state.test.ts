import { describe, expect, it } from "vitest";

import { deriveYaniMood, yaniMoodForPhase, yaniMoodValue } from "./state";

describe("Yani Neko visual state mapping", () => {
  it("respects alarm > scanning > busy > active > idle priority", () => {
    expect(deriveYaniMood({ active: true, busy: true, scanning: true, alarm: true })).toBe("alarm");
    expect(deriveYaniMood({ active: true, busy: true, scanning: true })).toBe("scanning");
    expect(deriveYaniMood({ active: true, busy: true })).toBe("busy");
    expect(deriveYaniMood({ active: true })).toBe("active");
    expect(deriveYaniMood({ active: false })).toBe("idle");
  });

  it("maps every shared telemetry phase to one stable mood and shader value", () => {
    expect(["idle", "engaging", "scanning", "focused", "fault"].map((phase) =>
      yaniMoodForPhase(phase as Parameters<typeof yaniMoodForPhase>[0]),
    )).toEqual(["idle", "busy", "scanning", "active", "alarm"]);
    expect(["idle", "busy", "scanning", "active", "alarm"].map((mood) =>
      yaniMoodValue(mood as Parameters<typeof yaniMoodValue>[0]),
    )).toEqual([0, 1, 2, 3, 4]);
  });
});
