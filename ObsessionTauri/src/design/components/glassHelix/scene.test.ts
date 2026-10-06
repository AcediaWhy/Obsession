import { beforeEach, describe, expect, it, vi } from "vitest";
import { createGlassHelix } from "./scene";

const harness = vi.hoisted(() => ({
  render: vi.fn(), dispose: vi.fn(), lose: vi.fn(), dpr: vi.fn(),
  loop: { start: vi.fn(), dispose: vi.fn(), invalidate: vi.fn(), setPaused: vi.fn() },
  draw: null as null | ((dt: number, now: number) => void),
  quality: null as null | ((tier: "high" | "balanced" | "low") => void),
}));
vi.mock("../../render", () => ({ createRenderLoop: (draw: (dt: number, now: number) => void,
  options: { onQualityChange: typeof harness.quality }) => {
  harness.draw = draw; harness.quality = options.onQualityChange; return harness.loop;
} }));
vi.mock("three", async (original) => ({ ...await original<typeof import("three")>(),
  WebGLRenderer: class {
    debug = { onShaderError: null };
    info = { render: { calls: 2, triangles: 21170 } };
    setClearColor = vi.fn(); setSize = vi.fn(); compile = vi.fn();
    setPixelRatio = harness.dpr; render = harness.render;
    dispose = harness.dispose; forceContextLoss = harness.lose;
  },
}));

function canvasStub() {
  const listeners = new Map<string, (event: Event) => void>();
  const canvas = { width: 360, height: 144, dataset: {},
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 360, height: 144 }),
    addEventListener: (name: string, listener: (event: Event) => void) => listeners.set(name, listener),
    removeEventListener: (name: string) => listeners.delete(name),
  } as unknown as HTMLCanvasElement;
  return { canvas, listeners };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("window", { devicePixelRatio: 3 });
  vi.stubGlobal("document", { documentElement: {} });
  vi.stubGlobal("getComputedStyle", () => ({ getPropertyValue: () => "190 205 240" }));
  const Observer = class { observe() {} disconnect() {} };
  vi.stubGlobal("ResizeObserver", Observer); vi.stubGlobal("MutationObserver", Observer);
});

describe("glass helix resource and frame limits", () => {
  it("releases its context and stops pending draws exactly once", () => {
    const { canvas, listeners } = canvasStub();
    const scene = createGlassHelix(canvas, { stage: "searching", candidate: "a", motionOff: false });
    harness.draw!(1 / 60, 100);
    scene.dispose(); scene.dispose(); harness.draw!(1 / 60, 120);
    expect(harness.render).toHaveBeenCalledTimes(1);
    expect(harness.loop.dispose).toHaveBeenCalledTimes(1);
    expect(harness.dispose).toHaveBeenCalledTimes(1);
    expect(harness.lose).toHaveBeenCalledTimes(1);
    expect(listeners.size).toBe(0);
    expect([canvas.width, canvas.height]).toEqual([0, 0]);
  });
  it("bounds pixel density and draws at 30 fps on the low tier", () => {
    const { canvas } = canvasStub();
    const scene = createGlassHelix(canvas, { stage: "searching", candidate: "a", motionOff: false });
    expect(harness.dpr).toHaveBeenLastCalledWith(1.25);
    harness.quality!("low");
    expect(harness.dpr).toHaveBeenLastCalledWith(.8);
    for (let i = 1; i <= 4; i++) harness.draw!(1 / 60, i * 1000 / 60);
    expect(harness.render).toHaveBeenCalledTimes(2);
    scene.update({ stage: "confirming", candidate: "a", motionOff: true });
    expect(harness.loop.setPaused).toHaveBeenLastCalledWith(true);
    scene.dispose();
  });
  it("falls back and frees resources after WebGL context loss", () => {
    const { canvas, listeners } = canvasStub(); const fallback = vi.fn();
    const scene = createGlassHelix(canvas, { stage: "searching", candidate: "a", motionOff: false }, fallback);
    listeners.get("webglcontextlost")!(new Event("webglcontextlost"));
    expect(fallback).toHaveBeenCalledOnce();
    expect(harness.loop.dispose).toHaveBeenCalledOnce();
    scene.dispose(); expect(harness.dispose).toHaveBeenCalledOnce();
  });
});
