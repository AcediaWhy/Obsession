import { frameScheduler, type FrameLoop, type FrameLoopOptions } from "./frameScheduler";

export type PointerFrame = {
  clientX: number;
  clientY: number;
  viewportX: number;
  viewportY: number;
  layoutChanged: boolean;
};

type PointerListener = (frame: PointerFrame) => void;

type PointerFrameScheduler = {
  createLoop(
    draw: (dt: number, now: number) => void,
    options?: FrameLoopOptions,
  ): FrameLoop;
};

export function normalizeViewportPointer(
  clientX: number,
  clientY: number,
  viewportWidth: number,
  viewportHeight: number,
): { x: number; y: number } {
  const width = Math.max(1, viewportWidth);
  const height = Math.max(1, viewportHeight);
  return {
    x: Math.max(-1, Math.min(1, (clientX / width - 0.5) * 2)),
    y: Math.max(-1, Math.min(1, (clientY / height - 0.5) * 2)),
  };
}

export class PointerFrameBus {
  private readonly listeners = new Set<PointerListener>();
  private readonly loop: FrameLoop;
  private clientX = 0;
  private clientY = 0;
  private viewportWidth = 1;
  private viewportHeight = 1;
  private layoutChanged = true;

  constructor(scheduler: PointerFrameScheduler) {
    this.loop = scheduler.createLoop(() => this.flush(), {
      paused: true,
      role: "secondary",
    });
  }

  subscribe(listener: PointerListener): () => void {
    const wasEmpty = this.listeners.size === 0;
    this.listeners.add(listener);
    if (wasEmpty) this.loop.start();
    this.loop.invalidate();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.loop.stop();
    };
  }

  updatePointer(
    clientX: number,
    clientY: number,
    viewportWidth: number,
    viewportHeight: number,
  ): void {
    this.clientX = clientX;
    this.clientY = clientY;
    this.viewportWidth = viewportWidth;
    this.viewportHeight = viewportHeight;
    this.loop.invalidate();
  }

  updateLayout(viewportWidth: number, viewportHeight: number): void {
    this.viewportWidth = viewportWidth;
    this.viewportHeight = viewportHeight;
    this.layoutChanged = true;
    this.loop.invalidate();
  }

  private flush(): void {
    const normalized = normalizeViewportPointer(
      this.clientX,
      this.clientY,
      this.viewportWidth,
      this.viewportHeight,
    );
    const frame: PointerFrame = {
      clientX: this.clientX,
      clientY: this.clientY,
      viewportX: normalized.x,
      viewportY: normalized.y,
      layoutChanged: this.layoutChanged,
    };
    this.layoutChanged = false;
    this.listeners.forEach((listener) => listener(frame));
  }
}

const pointerFrames = new PointerFrameBus(frameScheduler);
let domSubscribers = 0;

function onPointerMove(event: PointerEvent): void {
  pointerFrames.updatePointer(
    event.clientX,
    event.clientY,
    window.innerWidth,
    window.innerHeight,
  );
}

function onPointerLayoutChange(): void {
  pointerFrames.updateLayout(window.innerWidth, window.innerHeight);
}

function attachDomListeners(): void {
  window.addEventListener("pointermove", onPointerMove, { passive: true });
  window.addEventListener("resize", onPointerLayoutChange, { passive: true });
  window.addEventListener("scroll", onPointerLayoutChange, {
    capture: true,
    passive: true,
  });
  pointerFrames.updateLayout(window.innerWidth, window.innerHeight);
}

function detachDomListeners(): void {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("resize", onPointerLayoutChange);
  window.removeEventListener("scroll", onPointerLayoutChange, true);
}

export function subscribePointerFrame(listener: PointerListener): () => void {
  const unsubscribe = pointerFrames.subscribe(listener);
  domSubscribers += 1;
  if (domSubscribers === 1) attachDomListeners();

  return () => {
    unsubscribe();
    domSubscribers -= 1;
    if (domSubscribers === 0) detachDomListeners();
  };
}
