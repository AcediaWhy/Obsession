import { useEffect, useRef, useState } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { clamp, type PlaneIndex, setDither } from "./pixelart/pixelCore";
import { drawDynamicRoofScene, drawStaticRoofScene } from "./rooftopNightScene";
import "./SunkenStarFieldLab.css";

// Опорный кадр взят из настоящего окна Obsession (~994×677 CSS): при масштабе ×2
// это 480×330 арт-пикселей, и композиция крыши считалась именно под такую полосу.
// Опорные размеры нужны ТОЛЬКО чтобы выбрать шаг сетки — сам буфер подгоняется под
// окно. Наоборот, растянуть фиксированный буфер в 100%, нельзя: при нецелом
// масштабе блоки пикселей разъезжаются на ±1px, и хрусткость уходит вместе с ними.
//
// Композиция от этого не плывёт: всё, что важно, крыша мерит в пикселях от НИЗА
// кадра (см. roofGeometry), а небо — полосами в долях остатка.
const ART_WIDTH = 480;
const ART_HEIGHT = 330;
const MIN_SCALE = 2;
const MAX_SCALE = 6;

export type FieldMetrics = { scale: number; width: number; height: number };

type Props = {
  phase?: ObsessionVisualPhase;
  paused?: boolean;
  /** Инспектор: показать только один план глубины. */
  isolate?: PlaneIndex | null;
  /** Инспектор: выключить дизеринг, чтобы увидеть жёсткие стыки ступеней рампы. */
  dither?: boolean;
  /** Инспектор: зафиксировать масштаб вместо подбора под размер окна. */
  scale?: number | null;
  onMetrics?: (metrics: FieldMetrics) => void;
};

export function SunkenStarFieldLab({
  phase = "idle",
  paused = false,
  isolate = null,
  dither = true,
  scale = null,
  onMetrics,
}: Props) {
  const fieldRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const metricsCallback = useRef(onMetrics);
  metricsCallback.current = onMetrics;
  const [metrics, setMetrics] = useState<FieldMetrics>({
    scale: 2,
    width: ART_WIDTH,
    height: ART_HEIGHT,
  });

  useEffect(() => {
    const field = fieldRef.current;
    const canvas = canvasRef.current;
    if (!field || !canvas) return undefined;

    const context = canvas.getContext("2d", { alpha: false });
    if (!context) return undefined;
    const staticCanvas = document.createElement("canvas");
    const staticContext = staticCanvas.getContext("2d", { alpha: false });
    if (!staticContext) return undefined;

    // Тумблер дизеринга живёт в модуле pixelCore: сцена трогает его в сотне мест,
    // и протаскивать флаг параметром через каждый хелпер значило бы зашумить весь
    // рендерер ради одной кнопки инспектора. Поле в лабе одно, гонки нет.
    setDither(dither);

    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const motionPaused = paused || reduceMotion;
    let animationFrame = 0;
    let lastFrame = -1;
    let bufferWidth = 0;
    let bufferHeight = 0;
    let pixelScale = 0;

    const resize = () => {
      const rect = field.getBoundingClientRect();
      const ratio = window.devicePixelRatio || 1;
      const deviceWidth = Math.max(1, rect.width * ratio);
      const deviceHeight = Math.max(1, rect.height * ratio);
      const fit = Math.min(deviceWidth / ART_WIDTH, deviceHeight / ART_HEIGHT);
      const nextScale = clamp(Math.round(scale ?? fit), MIN_SCALE, MAX_SCALE);
      const nextWidth = Math.max(1, Math.ceil(deviceWidth / nextScale));
      const nextHeight = Math.max(1, Math.ceil(deviceHeight / nextScale));
      if (nextScale === pixelScale && nextWidth === bufferWidth && nextHeight === bufferHeight) return;

      pixelScale = nextScale;
      bufferWidth = nextWidth;
      bufferHeight = nextHeight;
      canvas.width = bufferWidth;
      canvas.height = bufferHeight;
      staticCanvas.width = bufferWidth;
      staticCanvas.height = bufferHeight;
      // Канвас чуть больше поля на дробный остаток — лишнее срезает overflow.
      canvas.style.width = `${(bufferWidth * pixelScale) / ratio}px`;
      canvas.style.height = `${(bufferHeight * pixelScale) / ratio}px`;
      context.imageSmoothingEnabled = false;
      staticContext.imageSmoothingEnabled = false;
      drawStaticRoofScene(staticContext, bufferWidth, bufferHeight, { isolate });
      lastFrame = -1;
      const next = { scale: pixelScale, width: bufferWidth, height: bufferHeight };
      setMetrics(next);
      metricsCallback.current?.(next);
    };

    const render = (frame: number) => {
      resize();
      context.drawImage(staticCanvas, 0, 0);
      drawDynamicRoofScene(context, bufferWidth, bufferHeight, phase, frame, { isolate });
    };

    const tick = (time: number) => {
      // Ступенчатые часы арта: 90 ms на кадр (~11 fps). Плавная анимация на
      // пиксельном спрайте читается как дрожание — R8.
      const frame = motionPaused ? 0 : Math.floor(time / 90);
      if (frame !== lastFrame) {
        render(frame);
        lastFrame = frame;
      }
      if (!motionPaused) animationFrame = window.requestAnimationFrame(tick);
    };

    const observer = new ResizeObserver(() => {
      resize();
      render(lastFrame < 0 ? 0 : lastFrame);
    });
    observer.observe(field);
    resize();
    tick(0);

    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(animationFrame);
    };
  }, [dither, isolate, paused, phase, scale]);

  return (
    <div
      aria-hidden="true"
      className="sunken-star-field-lab"
      data-art-size={`${metrics.width}x${metrics.height}`}
      data-isolate={isolate ?? undefined}
      data-motion={paused ? "paused" : "running"}
      data-phase={phase}
      data-pixel-scale={metrics.scale}
      data-renderer="procedural-canvas"
      data-sunken-star-field-lab
      ref={fieldRef}
    >
      <canvas className="sunken-star-field-lab__canvas" ref={canvasRef} />
    </div>
  );
}
