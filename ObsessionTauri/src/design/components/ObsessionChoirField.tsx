import { useEffect, useRef, useState } from "react";

import { subscribePointerFrame } from "../pointerBus";
import { createRenderLoop, useMotionOff, type RenderLoop, type QualityTier } from "../render";
import { ObsessionChoirFallback, type ObsessionChoirSceneProps } from "./ObsessionChoirFallback";
import { obsessionChoirFocusForScreen } from "./obsessionChoir/layout";
import { sampleObsessionChoirMotion, smoothObsessionChoirMotion } from "./obsessionChoir/motion";
import { readPanelLenses } from "./obsessionChoir/panelLenses";
import { ObsessionChoirPipeline } from "./obsessionChoir/pipeline";
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
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const motionOff = useMotionOff();
  const stateRef = useRef({ phase, screen, paused, motionOff, forceRitual, forcedQualityTier, lensRoot });
  stateRef.current = { phase, screen, paused, motionOff, forceRitual, forcedQualityTier, lensRoot };
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let pipeline: ObsessionChoirPipeline;
    try {
      pipeline = new ObsessionChoirPipeline(
        canvas,
        obsessionChoirQuality(stateRef.current.forcedQualityTier ?? "high"),
      );
    } catch (error) {
      console.warn("Black Choir WebGL failed; using the vector fallback", error);
      setFailed(true);
      return;
    }
    let loop: RenderLoop | null = null;
    let width = 1;
    let height = 1;
    let pointerX = 0;
    let pointerY = 0;
    let previousPhase = stateRef.current.phase;
    let phaseStartedAt = performance.now();
    let displayedMotion = sampleObsessionChoirMotion({
      time: phaseStartedAt / 1000,
      phase: previousPhase,
      phaseAge: 0,
      forceRitual: stateRef.current.forceRitual,
    });
    let schedulerQuality: QualityTier = "high";
    let appliedQuality = stateRef.current.forcedQualityTier ?? schedulerQuality;
    let renderBudget = 1;
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

    loop = createRenderLoop((dt, now) => {
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
      if (state.phase !== previousPhase) {
        previousPhase = state.phase;
        phaseStartedAt = now;
      }
      const focus = obsessionChoirFocusForScreen(state.screen);
      const targetMotion = sampleObsessionChoirMotion({
        time: now / 1000,
        phase: state.phase,
        phaseAge: Math.max(0, now - phaseStartedAt) / 1000,
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
        time: now / 1000,
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
      pipeline.destroy();
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
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      data-choir-field
      data-phase={phase}
      data-quality={forcedQualityTier ?? "adaptive"}
      data-motion={paused ? "still" : "running"}
      className="absolute inset-0 h-full w-full"
    />
  );
}
