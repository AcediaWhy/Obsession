import { describe, expect, it, vi } from "vitest";

import {
  PointerFrameBus,
  normalizeViewportPointer,
  type PointerFrame,
} from "./pointerBus";
import type { FrameLoop, FrameLoopOptions } from "./frameScheduler";

function createBusHarness() {
  let draw: ((dt: number, now: number) => void) | null = null;
  const loop: FrameLoop = {
    start: vi.fn(),
    stop: vi.fn(),
    setPaused: vi.fn(),
    invalidate: vi.fn(),
    dispose: vi.fn(),
  };
  const scheduler = {
    createLoop(
      nextDraw: (dt: number, now: number) => void,
      _options?: FrameLoopOptions,
    ) {
      draw = nextDraw;
      return loop;
    },
  };
  const bus = new PointerFrameBus(scheduler);
  return {
    bus,
    loop,
    flush: () => {
      if (!draw) throw new Error("Pointer loop was not created");
      draw(0, 0);
    },
  };
}

describe("PointerFrameBus", () => {
  it("normalizes viewport coordinates and clamps points outside the window", () => {
    expect(normalizeViewportPointer(500, 250, 1000, 500)).toEqual({ x: 0, y: 0 });
    expect(normalizeViewportPointer(-50, 900, 1000, 500)).toEqual({ x: -1, y: 1 });
  });

  it("coalesces pointer samples and exposes layout invalidation once", () => {
    const { bus, loop, flush } = createBusHarness();
    const frames: PointerFrame[] = [];
    const unsubscribe = bus.subscribe((frame) => frames.push(frame));

    bus.updatePointer(10, 20, 100, 100);
    bus.updatePointer(75, 25, 100, 100);
    bus.updateLayout(200, 100);

    expect(frames).toEqual([]);
    expect(loop.invalidate).toHaveBeenCalledTimes(4);

    flush();
    expect(frames).toEqual([
      {
        clientX: 75,
        clientY: 25,
        viewportX: -0.25,
        viewportY: -0.5,
        layoutChanged: true,
      },
    ]);

    flush();
    expect(frames[1]?.layoutChanged).toBe(false);

    unsubscribe();
    expect(loop.stop).toHaveBeenCalledOnce();
  });
});
