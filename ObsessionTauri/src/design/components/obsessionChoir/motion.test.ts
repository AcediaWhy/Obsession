import { describe, expect, it } from "vitest";

import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import {
  choirBlinkAt,
  choirBlinkWindow,
  choirRitualAt,
  choirRitualWindow,
  sampleObsessionChoirMotion,
  smoothObsessionChoirMotion,
} from "./motion";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];

describe("Black Choir motion director", () => {
  it("is deterministic and keeps every channel in range", () => {
    for (const phase of PHASES) {
      const input = { time: 42.75, phase, phaseAge: 0.82 } as const;
      const first = sampleObsessionChoirMotion(input);
      expect(sampleObsessionChoirMotion(input)).toEqual(first);
      expect(first.masterGazeX).toBeGreaterThanOrEqual(-1);
      expect(first.masterGazeX).toBeLessThanOrEqual(1);
      expect(first.masterGazeY).toBeGreaterThanOrEqual(-1);
      expect(first.masterGazeY).toBeLessThanOrEqual(1);
      for (const value of Object.values(first).slice(2)) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("places rituals 14–22 seconds apart and holds them for 1.1–1.5 seconds", () => {
    const windows = Array.from({ length: 30 }, (_, cycle) => choirRitualWindow(cycle));
    for (const window of windows) {
      expect(window.duration).toBeGreaterThanOrEqual(1.1);
      expect(window.duration).toBeLessThanOrEqual(1.5);
    }
    for (let index = 1; index < windows.length; index += 1) {
      expect(windows[index].start - windows[index - 1].start).toBeGreaterThanOrEqual(14);
      expect(windows[index].start - windows[index - 1].start).toBeLessThanOrEqual(22);
    }
    const window = windows[8];
    expect(choirRitualAt(window.start + window.duration * 0.25)).toBeGreaterThan(0.5);
    expect(choirRitualAt(window.start + window.duration + 0.1)).toBe(0);
  });

  it("adds deterministic single and double blinks 3.8–7.2 seconds apart", () => {
    const windows = Array.from({ length: 40 }, (_, cycle) => choirBlinkWindow(cycle));
    for (let index = 1; index < windows.length; index += 1) {
      expect(windows[index].start - windows[index - 1].start).toBeGreaterThanOrEqual(3.8);
      expect(windows[index].start - windows[index - 1].start).toBeLessThanOrEqual(7.2);
    }
    const window = windows.find((candidate) => candidate.start > 0)!;
    expect(choirBlinkAt(window.start + window.duration * 0.43)).toBeGreaterThan(0.9);
    expect(choirBlinkAt(window.start + window.duration + 0.6)).toBe(0);
  });

  it("distinguishes ritual, scanning, focused and fault choreography", () => {
    const idle = sampleObsessionChoirMotion({ time: 3, phase: "idle", phaseAge: 3 });
    const ritual = sampleObsessionChoirMotion({ time: 3, phase: "idle", phaseAge: 3, forceRitual: true });
    const scanning = sampleObsessionChoirMotion({ time: 3, phase: "scanning", phaseAge: 1 });
    const focused = sampleObsessionChoirMotion({ time: 3, phase: "focused", phaseAge: 1 });
    const fault = sampleObsessionChoirMotion({ time: 3, phase: "fault", phaseAge: 1 });
    expect(ritual.chorusReveal).toBeGreaterThan(idle.chorusReveal);
    expect(ritual.chorusAlignment).toBe(1);
    expect(scanning.lineTension).toBeGreaterThan(focused.lineTension);
    expect(scanning.lineFlow).toBeGreaterThan(focused.lineFlow);
    expect(focused.chorusAlignment).toBeGreaterThan(idle.chorusAlignment);
    expect(fault.faultShear).toBeGreaterThan(0);
  });

  it("keeps the focused eye alive with saccades, body drift and full blinks", () => {
    const first = sampleObsessionChoirMotion({ time: 2.1, phase: "focused", phaseAge: 2.1 });
    const second = sampleObsessionChoirMotion({ time: 4.2, phase: "focused", phaseAge: 4.2 });
    expect(Math.abs(second.masterGazeX - first.masterGazeX)).toBeGreaterThan(0.04);
    expect(Math.abs(second.masterBodyX - first.masterBodyX)).toBeGreaterThan(0.02);

    const blink = choirBlinkWindow(3);
    const open = sampleObsessionChoirMotion({
      time: blink.start - 0.3,
      phase: "focused",
      phaseAge: blink.start - 0.3,
    });
    const closed = sampleObsessionChoirMotion({
      time: blink.start + blink.duration * 0.43,
      phase: "focused",
      phaseAge: blink.start + blink.duration * 0.43,
    });
    expect(closed.masterLidOpen).toBeLessThan(0.12);
    expect(open.masterLidOpen).toBeGreaterThan(0.8);
  });

  it("eases phase changes without snapping or overshooting", () => {
    const current = sampleObsessionChoirMotion({ time: 4, phase: "idle", phaseAge: 4 });
    const target = sampleObsessionChoirMotion({ time: 4, phase: "focused", phaseAge: 0 });
    expect(smoothObsessionChoirMotion(current, target, 0)).toEqual(current);

    const first = smoothObsessionChoirMotion(current, target, 1 / 60);
    const second = smoothObsessionChoirMotion(first, target, 1 / 60);
    expect(first).not.toEqual(current);
    expect(first).not.toEqual(target);
    for (const channel of Object.keys(current) as (keyof typeof current)[]) {
      const low = Math.min(current[channel], target[channel]);
      const high = Math.max(current[channel], target[channel]);
      expect(first[channel]).toBeGreaterThanOrEqual(low);
      expect(first[channel]).toBeLessThanOrEqual(high);
      expect(Math.abs(second[channel] - target[channel])).toBeLessThanOrEqual(
        Math.abs(first[channel] - target[channel]),
      );
    }
  });
});
