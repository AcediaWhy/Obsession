import { afterEach, describe, expect, it, vi } from "vitest";
import { ATMOSPHERE_DURATION, createAtmosphereController, type AtmosphereEvent } from "./themeAtmosphere";
import type { ObsessionVisualPhase } from "./obsessionVisualState";

function setup(phase: ObsessionVisualPhase = "idle") {
  vi.useFakeTimers();
  const events: (AtmosphereEvent | null)[] = [];
  const controller = createAtmosphereController({
    setTimeout: (callback, delay) => setTimeout(callback, delay),
    clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
    random: () => .5,
  }, (event) => events.push(event), phase);
  return { controller, events };
}
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe("theme atmosphere lifecycle", () => {
  it("sleeps between bounded events, using one timer and no overlapping gestures", () => {
    const { controller, events } = setup();
    controller.setEnabled(true);
    vi.advanceTimersByTime(49_999);
    expect(events.filter(Boolean)).toHaveLength(0);
    vi.advanceTimersByTime(1);
    expect(events[events.length - 1]).toMatchObject({ kind: "rare", duration: 14_000 });
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(ATMOSPHERE_DURATION.rare);
    expect(events[events.length - 1]).toBeNull();
    vi.advanceTimersByTime(49_999);
    expect(events.filter(Boolean)).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(events.filter(Boolean)).toHaveLength(2);
    controller.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels a running gesture and all timers when hidden or reduced motion is enabled", () => {
    const { controller, events } = setup();
    controller.setEnabled(true);
    controller.preview();
    expect(events[events.length - 1]?.kind).toBe("rare");
    controller.setEnabled(false);
    expect(events[events.length - 1]).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    controller.preview();
    controller.setPhase("focused");
    vi.advanceTimersByTime(300_000);
    expect(events.filter(Boolean)).toHaveLength(1);
    controller.setEnabled(true);
    expect(events[events.length - 1]).toBeNull();
    vi.advanceTimersByTime(49_999);
    expect(events.filter(Boolean)).toHaveLength(1);
    controller.dispose();
  });

  it("waits through busy/error states and reacts to completion without celebrating initial state", () => {
    const { controller, events } = setup("focused");
    controller.setEnabled(true);
    expect(events.filter(Boolean)).toHaveLength(0);
    controller.setPhase("engaging");
    expect(vi.getTimerCount()).toBe(0);
    controller.preview();
    vi.advanceTimersByTime(100_000);
    expect(events.filter(Boolean)).toHaveLength(0);
    controller.setPhase("focused");
    expect(events[events.length - 1]?.kind).toBe("activation");
    controller.setPhase("focused");
    expect(events.filter(Boolean)).toHaveLength(1);
    controller.setPhase("fault");
    expect(events[events.length - 1]).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    controller.dispose();
  });

  it("replaces a rare event with a state reaction and releases with a softer gesture", () => {
    const { controller, events } = setup();
    controller.setEnabled(true);
    controller.preview();
    controller.setPhase("focused");
    expect(events[events.length - 1]?.kind).toBe("activation");
    expect(vi.getTimerCount()).toBe(1);
    controller.setPhase("engaging");
    expect(events[events.length - 1]).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    controller.setPhase("idle");
    expect(events[events.length - 1]?.kind).toBe("release");
    vi.advanceTimersByTime(ATMOSPHERE_DURATION.release);
    expect(events[events.length - 1]).toBeNull();
    controller.dispose();
    const count = events.length;
    controller.preview();
    controller.setEnabled(true);
    controller.setPhase("focused");
    vi.advanceTimersByTime(300_000);
    expect(events).toHaveLength(count);
    expect(vi.getTimerCount()).toBe(0);
  });
});
