import { createGoldenMeadow, type GoldenMeadowOptions } from "./goldenMeadowScene.js";

export type MeadowRequest =
  | { type: "init"; canvas: OffscreenCanvas; scale: number }
  | { type: "resize"; scale: number }
  | { type: "frame"; time: number; options: GoldenMeadowOptions };

export type MeadowReply =
  | { type: "ready"; diagnostics: Record<string, string> }
  | { type: "frame"; time: number; renderMs: string }
  | { type: "error"; message: string };

let canvas: OffscreenCanvas | undefined;
let scene: ReturnType<typeof createGoldenMeadow> | undefined;
let time = 0;
let options: GoldenMeadowOptions = {};

// No timer lives in this worker. The app's visibility/reduced-motion scheduler
// requests frames, and its controller allows only one request in flight.
self.onmessage = (event: MessageEvent<MeadowRequest>) => {
  try {
    const message = event.data;
    if (message.type === "init" || message.type === "resize") {
      if (message.type === "init") canvas = message.canvas;
      if (!canvas) return;
      scene?.dispose();
      scene = createGoldenMeadow(canvas, { scale: message.scale, cacheBackground: true });
      scene.render(time, options);
      self.postMessage({ type: "ready", diagnostics: scene.diagnostics } satisfies MeadowReply);
    } else if (scene) {
      time = message.time;
      options = message.options;
      scene.render(time, options);
      self.postMessage({ type: "frame", time, renderMs: scene.diagnostics.renderMs } satisfies MeadowReply);
    }
  } catch (error) {
    self.postMessage({ type: "error", message: String(error) } satisfies MeadowReply);
  }
};
