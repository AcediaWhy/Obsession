import { drawDynamicMakeItScene, drawStaticMakeItScene, MAKE_IT_HEIGHT } from "./design/components/axolotl/makeItPixelScene";

const canvas = document.querySelector<HTMLCanvasElement>("#scene");
if (!canvas) throw new Error("Make It scene canvas is missing");
const context = canvas.getContext("2d", { alpha: false });
if (!context) throw new Error("2D canvas context is unavailable");
const sceneCanvas: HTMLCanvasElement = canvas;
const sceneContext: CanvasRenderingContext2D = context;

const staticCanvas = document.createElement("canvas");
const staticContext = staticCanvas.getContext("2d", { alpha: false });
if (!staticContext) throw new Error("Static 2D canvas context is unavailable");
const sceneStaticContext: CanvasRenderingContext2D = staticContext;

let pixelScale = 0;
let lastFrame = -1;

function resize() {
  const ratio = window.devicePixelRatio || 1;
  const nextScale = Math.max(1, Math.round(window.innerHeight * ratio / MAKE_IT_HEIGHT));
  const width = Math.ceil(window.innerWidth * ratio / nextScale);
  const height = Math.ceil(window.innerHeight * ratio / nextScale);
  if (sceneCanvas.width === width && sceneCanvas.height === height && pixelScale === nextScale) return;
  pixelScale = nextScale;
  sceneCanvas.width = width;
  sceneCanvas.height = height;
  staticCanvas.width = width;
  staticCanvas.height = height;
  sceneCanvas.style.width = `${width * nextScale / ratio}px`;
  sceneCanvas.style.height = `${height * nextScale / ratio}px`;
  sceneContext.imageSmoothingEnabled = false;
  sceneStaticContext.imageSmoothingEnabled = false;
  drawStaticMakeItScene(sceneStaticContext, width, height);
  lastFrame = -1;
}

function tick(time: number) {
  resize();
  const frame = Math.floor(time / 110);
  if (frame !== lastFrame) {
    sceneContext.drawImage(staticCanvas, 0, 0);
    drawDynamicMakeItScene(sceneContext, sceneCanvas.width, sceneCanvas.height, "idle", frame);
    lastFrame = frame;
  }
  window.requestAnimationFrame(tick);
}

window.addEventListener("resize", resize);
resize();
tick(0);
