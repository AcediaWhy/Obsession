import { describe, expect, it } from "vitest";
import { patternOptions, patternStage } from "./state";
import type { Zapret2PickerProgress } from "../../../lib/tauri";

const progress:Zapret2PickerProgress={category:"discord",transport:"tls",current:1,total:5,
  profileId:"candidate",label:"Candidate",phase:"screening"};
describe("dot silhouette follows confirmed picker events",()=>{
  it("does not finish a silhouette while probing or restoring",()=>{
    for(const phase of ["screening","confirmation","composition","restoring"] as const) {
      expect(patternOptions(patternStage({...progress,phase},false,false,true,false)).morph).toBeLessThan(1);
    }
  });
  it("gives cancellation priority even if a confirmed result races it",()=>{
    expect(patternStage(progress,true,true,true,false)).toBe("cancelled");
    expect(patternStage(progress,false,true,true,true)).toBe("cancelled");
    expect(patternOptions(patternStage(progress,true,false,true,false)).interactive).toBe(false);
  });
  it("assembles the complete icon only after a confirmed completion",()=>{
    expect(patternOptions(patternStage(progress,false,true,false,false)).morph).toBe(0);
    expect(patternOptions(patternStage(progress,false,true,true,false)).morph).toBe(1);
  });
});
