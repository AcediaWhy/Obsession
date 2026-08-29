import { useEffect, useRef, useState } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { subscribePointerFrame } from "../pointerBus";
import { createRenderLoop, useMotionOff, type QualityTier, type RenderLoop } from "../render";
import { YaniNekoFallback } from "./YaniNekoFallback";
import { yaniFieldSession } from "./yanineko/fieldSession";
import { readYaniPanelLenses } from "./yanineko/panelLenses";
import type { YaniNekoPipeline } from "./yanineko/pipeline";
import { yaniQuality } from "./yanineko/quality";

type Props = {
  phase?: ObsessionVisualPhase;
  screen?: string;
  paused?: boolean;
  qualityTier?: QualityTier;
  forceFallback?: boolean;
};

function compositionForScreen(screen: string): { x: number; y: number } {
  if (screen === "settings") return { x: -0.018, y: 0 };
  if (screen === "dpi") return { x: 0.012, y: 0.006 };
  if (screen === "telegram") return { x: -0.01, y: -0.004 };
  if (screen === "ai") return { x: 0.006, y: 0 };
  return { x: 0, y: 0 };
}
export function YaniNekoField({
  phase = "idle",
  screen = "overview",
  paused = false,
  qualityTier,
  forceFallback = false,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const motionOff = useMotionOff();
  const stateRef = useRef({ phase, screen, paused, motionOff, qualityTier });
  stateRef.current = { phase, screen, paused, motionOff, qualityTier };
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (forceFallback) return;
    const container = containerRef.current;
    if (!container) return;
    // Канвас+контекст персистентны (gl/persistentGlSession): unmount поля
    // ОБЯЗАН оставлять их жить, иначе каждое переключение темы снова гонит
    // цикл создания/потери WebGL-контекста.
    let canvas: HTMLCanvasElement;
    let pipeline: YaniNekoPipeline;
    try {
      const session = yaniFieldSession.acquire();
      canvas = session.canvas;
      pipeline = session.pipeline;
    } catch (error) {
      console.warn("Yani Neko WebGL failed; using the cinematic Canvas fallback", error);
      setFailed(true);
      return;
    }
    canvas.className = "block h-full w-full";
    container.appendChild(canvas);
    pipeline.setQuality(yaniQuality(stateRef.current.qualityTier ?? "high"));
    pipeline.resetFeedback();

    let width = 1;
    let height = 1;
    let dpr = 1;
    let panels = readYaniPanelLenses(canvas.getBoundingClientRect(), yaniQuality(stateRef.current.qualityTier ?? "high").panelCount);
    let animationTime = 0;
    let pointerX = 0.5;
    let pointerY = 0.5;
    let previousPointerX = pointerX;
    let previousPointerY = pointerY;
    let pointerVx = 0;
    let pointerVy = 0;
    let schedulerQuality: QualityTier = "high";
    let appliedQuality = stateRef.current.qualityTier ?? schedulerQuality;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      width = Math.max(1, Math.round(rect.width * dpr));
      height = Math.max(1, Math.round(rect.height * dpr));
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      panels = readYaniPanelLenses(rect, yaniQuality(appliedQuality).panelCount);
    };
    resize();
    const resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize);
    resizeObserver?.observe(canvas);
    let pendingPanelRead = 0;
    const schedulePanelRead = () => {
      if (pendingPanelRead) cancelAnimationFrame(pendingPanelRead);
      pendingPanelRead = requestAnimationFrame(() => {
        pendingPanelRead = 0;
        resize();
      });
    };
    const mutationObserver = typeof MutationObserver === "undefined"
      ? null
      : new MutationObserver(schedulePanelRead);
    mutationObserver?.observe(document.body, { childList: true, subtree: true });

    const unsubscribePointer = subscribePointerFrame((pointer) => {
      const nextX = (pointer.viewportX + 1) * 0.5;
      const nextY = 1 - (pointer.viewportY + 1) * 0.5;
      pointerVx = nextX - previousPointerX;
      pointerVy = nextY - previousPointerY;
      previousPointerX = nextX;
      previousPointerY = nextY;
      pointerX = nextX;
      pointerY = nextY;
      if (pointer.layoutChanged) resize();
    });

    const onContextLost = (event: Event) => {
      event.preventDefault();
      loopRef.current?.dispose();
      pipeline.abandonAfterContextLoss();
      setFailed(true);
    };
    canvas.addEventListener("webglcontextlost", onContextLost);

    const loop = createRenderLoop((dt) => {
      const state = stateRef.current;
      const nextQuality = state.qualityTier ?? schedulerQuality;
      if (nextQuality !== appliedQuality) {
        appliedQuality = nextQuality;
        pipeline.setQuality(yaniQuality(appliedQuality));
        resize();
      }
      const still = state.paused || state.motionOff;
      const effectiveDt = still ? 0 : dt;
      animationTime += effectiveDt;
      pointerVx *= Math.exp(-dt * 9);
      pointerVy *= Math.exp(-dt * 9);
      const shift = compositionForScreen(state.screen);
      pipeline.render(width, height, {
        time: animationTime,
        dt: effectiveDt,
        phase: state.phase,
        pointerX,
        pointerY,
        pointerVx,
        pointerVy,
        sceneShiftX: shift.x,
        sceneShiftY: shift.y,
        panels,
      });
    }, {
      role: "field",
      paused: stateRef.current.paused,
      onQualityChange: (tier) => {
        schedulerQuality = tier;
      },
    });
    loopRef.current = loop;
    loop.start();

    return () => {
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
      if (pendingPanelRead) cancelAnimationFrame(pendingPanelRead);
      unsubscribePointer();
      canvas.removeEventListener("webglcontextlost", onContextLost);
      loop.dispose();
      loopRef.current = null;
      // Контекст остаётся в персистентной сессии; обнуляем только drawing
      // buffer, чтобы скрытый канвас не держал полноэкранный буфер в трее.
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [forceFallback]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [motionOff, paused, phase, qualityTier, screen]);

  if (forceFallback || failed) {
    return <YaniNekoFallback phase={phase} screen={screen} paused={paused} qualityTier={qualityTier} />;
  }

  return (
    <div
      ref={containerRef}
      aria-hidden="true"
      data-yani-field
      data-phase={phase}
      data-quality={qualityTier ?? "adaptive"}
      data-motion={paused ? "still" : "running"}
      className="absolute inset-0"
    />
  );
}
