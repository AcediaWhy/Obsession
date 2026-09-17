import type { GoldenMeadowOptions } from "./goldenMeadowScene.js";
import type { MeadowReply, MeadowRequest } from "./goldenMeadow.worker";

export function goldenMeadowScale(width: number, height: number, dpr = 1): number {
  // Bound raster memory/bandwidth even on a maximized HiDPI window.
  return Math.min(1.5, Math.max(1, Math.ceil(Math.max(width / 735, height / 505) * dpr * 4) / 4));
}

export function createGoldenMeadowRenderer(
  canvas: HTMLCanvasElement,
  onError: (message: string) => void,
) {
  const bounds = canvas.getBoundingClientRect();
  let requestedScale = goldenMeadowScale(bounds.width, bounds.height, window.devicePixelRatio);
  let appliedScale = requestedScale;
  let busy = true;
  let disposed = false;
  let lastTelemetry = -1;
  const worker = new Worker(new URL("./goldenMeadow.worker.ts", import.meta.url), { type: "module" });
  const post = (message: MeadowRequest, transfer: Transferable[] = []) => worker.postMessage(message, transfer);
  const fail = (message: string) => {
    if (disposed) return;
    disposed = true;
    worker.terminate();
    onError(message);
  };
  const flushResize = () => {
    if (disposed || busy || requestedScale === appliedScale) return;
    appliedScale = requestedScale;
    busy = true;
    post({ type: "resize", scale: appliedScale });
  };
  worker.onerror = (event) => { event.preventDefault(); fail(event.message); };
  worker.onmessageerror = () => fail("Golden Meadow worker message could not be decoded");
  worker.onmessage = (event: MessageEvent<MeadowReply>) => {
    if (disposed) return;
    const message = event.data;
    if (message.type === "error") { fail(message.message); return; }
    busy = false;
    if (message.type === "ready") {
      Object.assign(canvas.dataset, message.diagnostics);
      canvas.dataset.renderer = "worker";
      canvas.style.opacity = "1";
    } else if (message.time - lastTelemetry >= 1) {
      lastTelemetry = message.time;
      canvas.dataset.time = message.time.toFixed(3);
      canvas.dataset.renderMs = message.renderMs;
    }
    flushResize();
  };
  try {
    const offscreen = canvas.transferControlToOffscreen();
    post({ type: "init", canvas: offscreen, scale: requestedScale }, [offscreen]);
  } catch (error) {
    worker.terminate();
    throw error;
  }
  return {
    render(time: number, options: GoldenMeadowOptions = {}) {
      if (disposed || busy) return;
      busy = true;
      post({ type: "frame", time, options });
    },
    resize(width: number, height: number) {
      requestedScale = goldenMeadowScale(width, height, window.devicePixelRatio);
      flushResize();
    },
    dispose() {
      disposed = true;
      worker.onmessage = worker.onerror = worker.onmessageerror = null;
      worker.terminate();
    },
  };
}
