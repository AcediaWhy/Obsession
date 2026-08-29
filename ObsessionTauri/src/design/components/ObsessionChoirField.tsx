import { useEffect, useRef, useState } from "react";

import { subscribePointerFrame } from "../pointerBus";
import { createRenderLoop, useMotionOff, type RenderLoop, type QualityTier } from "../render";
import { ObsessionChoirFallback, type ObsessionChoirSceneProps } from "./ObsessionChoirFallback";
import { choirFieldSession } from "./obsessionChoir/fieldSession";
import { obsessionChoirFocusForScreen } from "./obsessionChoir/layout";
import { sampleObsessionChoirMotion, smoothObsessionChoirMotion } from "./obsessionChoir/motion";
import { readPanelLenses } from "./obsessionChoir/panelLenses";
import type { ObsessionChoirPipeline } from "./obsessionChoir/pipeline";
import { obsessionChoirQuality } from "./obsessionChoir/quality";

type Props = ObsessionChoirSceneProps & {
  qualityTier?: QualityTier;
  lensRoot?: ParentNode;
};

export function ObsessionChoirField({
  phase,
  screen,
  paused = false,
  forceRitual = false,
  qualityTier: forcedQualityTier,
  lensRoot,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const motionOff = useMotionOff();
  const stateRef = useRef({ phase, screen, paused, motionOff, forceRitual, forcedQualityTier, lensRoot });
  stateRef.current = { phase, screen, paused, motionOff, forceRitual, forcedQualityTier, lensRoot };
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    // Канвас+контекст персистентны (gl/persistentGlSession): unmount поля
    // оставляет их жить в сессии темы.
    let canvas: HTMLCanvasElement;
    let pipeline: ObsessionChoirPipeline;
    try {
      const session = choirFieldSession.acquire();
      canvas = session.canvas;
      pipeline = session.pipeline;
    } catch (error) {
      console.warn("Black Choir WebGL failed; using the vector fallback", error);
      setFailed(true);
      return;
    }
    canvas.className = "block h-full w-full";
    container.appendChild(canvas);
    pipeline.setQuality(obsessionChoirQuality(stateRef.current.forcedQualityTier ?? "high"));
    let loop: RenderLoop | null = null;
    let width = 1;
    let height = 1;
    let pointerX = 0;
    let pointerY = 0;
    let previousPhase = stateRef.current.phase;
    let phaseAge = 0;
    let animationTime = 0;
    let displayedMotion = sampleObsessionChoirMotion({
      time: animationTime,
      phase: previousPhase,
      phaseAge: 0,
      forceRitual: stateRef.current.forceRitual,
    });
    let schedulerQuality: QualityTier = "high";
    let appliedQuality = stateRef.current.forcedQualityTier ?? schedulerQuality;
    let renderBudget = 1 / obsessionChoirQuality(appliedQuality).targetFps;
    let panels = readPanelLenses(stateRef.current.lensRoot ?? document, canvas.getBoundingClientRect(), 12);

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      width = Math.max(1, Math.round(rect.width * dpr));
      height = Math.max(1, Math.round(rect.height * dpr));
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      panels = readPanelLenses(stateRef.current.lensRoot ?? document, rect, 12);
    };
    resize();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize);
    observer?.observe(canvas);
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
    const onContextLost = (event: Event) => {
      event.preventDefault();
      loop?.dispose();
      pipeline.abandonAfterContextLoss();
      setFailed(true);
    };
    canvas.addEventListener("webglcontextlost", onContextLost);
    const unsubscribePointer = subscribePointerFrame((pointer) => {
      pointerX = pointer.viewportX;
      pointerY = pointer.viewportY;
      if (pointer.layoutChanged) resize();
    });

    loop = createRenderLoop((dt) => {
      const state = stateRef.current;
      const nextQuality = state.forcedQualityTier ?? schedulerQuality;
      if (nextQuality !== appliedQuality) {
        appliedQuality = nextQuality;
        pipeline.setQuality(obsessionChoirQuality(appliedQuality));
        renderBudget = 1 / obsessionChoirQuality(appliedQuality).targetFps;
      }
      const quality = obsessionChoirQuality(appliedQuality);
      renderBudget += dt;
      if (renderBudget + 0.0001 < 1 / quality.targetFps) return;
      const frameDt = renderBudget;
      renderBudget = 0;
      const still = state.paused || state.motionOff;
      const effectiveDt = still ? 0 : frameDt;
      animationTime += effectiveDt;
      if (state.phase !== previousPhase) {
        previousPhase = state.phase;
        phaseAge = 0;
      } else {
        phaseAge += effectiveDt;
      }
      const focus = obsessionChoirFocusForScreen(state.screen);
      const targetMotion = sampleObsessionChoirMotion({
        time: animationTime,
        phase: state.phase,
        phaseAge,
        forceRitual: state.forceRitual,
      });
      displayedMotion = state.paused || state.motionOff
        ? targetMotion
        : smoothObsessionChoirMotion(
          displayedMotion,
          targetMotion,
          frameDt,
          state.phase === "fault" ? 8 : 5.75,
        );
      pipeline.render(width, height, {
        ...displayedMotion,
        time: animationTime,
        phase: state.phase,
        focusX: focus.x,
        focusY: focus.y,
        pointerX,
        pointerY,
        panels,
      });
    }, {
      role: "field",
      fps: 60,
      paused: stateRef.current.paused,
      onQualityChange: (tier) => {
        schedulerQuality = tier;
      },
    });
    loopRef.current = loop;
    loop.start();

    return () => {
      observer?.disconnect();
      mutationObserver?.disconnect();
      if (pendingPanelRead) cancelAnimationFrame(pendingPanelRead);
      unsubscribePointer();
      canvas.removeEventListener("webglcontextlost", onContextLost);
      loop?.dispose();
      loopRef.current = null;
      // Контекст остаётся в персистентной сессии; drawing buffer отпускаем.
      canvas.width = 0;
      canvas.height = 0;
    };
  }, []);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [forceRitual, forcedQualityTier, motionOff, paused, phase, screen]);

  if (failed) {
    return <ObsessionChoirFallback phase={phase} screen={screen} paused={paused} forceRitual={forceRitual} />;
  }
  return (
    <div
      ref={containerRef}
      aria-hidden="true"
      data-choir-field
      data-phase={phase}
      data-quality={forcedQualityTier ?? "adaptive"}
      data-motion={paused ? "still" : "running"}
      className="absolute inset-0"
    />
  );
}
