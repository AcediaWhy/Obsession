import { afterEach, describe, expect, it, vi } from "vitest";
import { createGoldenMeadowRenderer, goldenMeadowScale } from "./goldenMeadowRenderer";

class FakeWorker {
  static instances: FakeWorker[] = [];
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onmessageerror: (() => void) | null = null;
  postMessage = vi.fn();
  terminate = vi.fn();
  constructor() { FakeWorker.instances.push(this); }
  reply(data: unknown) { this.onmessage?.({ data }); }
}

function setup() {
  FakeWorker.instances = [];
  vi.stubGlobal("Worker", FakeWorker);
  vi.stubGlobal("window", { devicePixelRatio: 1 });
  const canvas = { getBoundingClientRect: () => ({width:735,height:505}),
    transferControlToOffscreen: vi.fn(() => ({})), dataset: {}, style: {} } as unknown as HTMLCanvasElement;
  const fail = vi.fn();
  const renderer = createGoldenMeadowRenderer(canvas, fail);
  return {renderer, worker:FakeWorker.instances[0], canvas, fail};
}
afterEach(() => vi.unstubAllGlobals());

describe("Golden Meadow worker lifecycle", () => {
  it("bounds raster memory on large HiDPI displays", () => {
    expect(goldenMeadowScale(3840,2160,2)).toBe(1.5);
    expect(goldenMeadowScale(735,505,1)).toBe(1);
  });
  it("never queues frames while initialization or a previous frame is unfinished", () => {
    const {renderer,worker}=setup();
    renderer.render(1); renderer.render(2);
    expect(worker.postMessage).toHaveBeenCalledTimes(1);
    worker.reply({type:"ready",diagnostics:{}});
    renderer.render(3); renderer.render(4);
    expect(worker.postMessage).toHaveBeenCalledTimes(2);
    worker.reply({type:"frame",time:3,renderMs:"2"});
    renderer.render(5);
    expect(worker.postMessage.mock.calls[worker.postMessage.mock.calls.length - 1]?.[0].time).toBe(5);
    renderer.dispose();
  });
  it("coalesces resize requests and releases the worker on unmount", () => {
    const {renderer,worker,canvas}=setup();
    renderer.resize(850,580); renderer.resize(1100,750);
    worker.reply({type:"ready",diagnostics:{renderScale:"1"}});
    expect(worker.postMessage.mock.calls[worker.postMessage.mock.calls.length - 1]?.[0]).toEqual({type:"resize",scale:1.5});
    renderer.dispose();
    expect(worker.terminate).toHaveBeenCalledOnce();
    const sent=worker.postMessage.mock.calls.length;
    renderer.render(10); renderer.resize(735,505);
    worker.reply({type:"ready",diagnostics:{renderScale:"2"}});
    expect(worker.postMessage).toHaveBeenCalledTimes(sent);
    expect(canvas.dataset.renderScale).toBe("1");
  });
  it("terminates a failed worker before selecting the still fallback", () => {
    const {worker,fail,renderer}=setup();
    worker.reply({type:"error",message:"unsupported"});
    expect(worker.terminate).toHaveBeenCalledOnce();
    expect(fail).toHaveBeenCalledWith("unsupported");
    renderer.render(3);
    expect(worker.postMessage).toHaveBeenCalledTimes(1);
  });
});
