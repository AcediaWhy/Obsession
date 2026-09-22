import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dpiTestBlockedReason } from "./dpiTestAvailability";
const idle = { available: true, transitioning: false, adaptiveBusy: false, active: false, testing: false, categories: 2 };
describe("legacy test availability", () => {
  it("uses protected DPI capability, not the retired always-false runtime gate", () => {
    const source = readFileSync(new URL("../screens/Dpi.tsx", import.meta.url), "utf8");
    expect(source).not.toContain("state.protectedRuntimeAvailable");
    expect(dpiTestBlockedReason(idle)).toBe("");
  });
  it("explains each blocked state", () => {
    for (const patch of [{available:false}, {transitioning:true}, {adaptiveBusy:true}, {active:true}, {categories:0}]) {
      expect(dpiTestBlockedReason({...idle,...patch})).not.toBe("");
    }
  });
  it("does not mislabel the test's temporary runtime as user protection", () => {
    expect(dpiTestBlockedReason({...idle, active:true, testing:true})).toBe("");
  });
});
