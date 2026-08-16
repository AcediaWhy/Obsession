import { useEffect, useRef, useState } from "react";

import {
  obsessionFocusForScreen,
  obsessionPhaseValue,
  type ObsessionVisualPhase,
} from "../obsessionVisualState";
import { subscribePointerFrame } from "../pointerBus";
import { createRenderLoop, useMotionOff, type RenderLoop } from "../render";
import { ObsessionFallback, type ObsessionSceneProps } from "./ObsessionFallback";
import { ObsessionPipeline } from "./obsession/pipeline";
import { obsessionQualityProfile } from "./obsession/quality";
import { sampleObsessionEyeMotion } from "./obsession/eyeMotion";

function damp(current: number, target: number, speed: number, dt: number): number {
  return current + (target - current) * (1 - Math.exp(-speed * dt));
}

function scanningOffset(time: number): { x: number; y: number } {
  const stops = [
    { x: -0.035, y: 0.018 },
    { x: 0.026, y: -0.022 },
    { x: 0.014, y: 0.031 },
    { x: -0.021, y: -0.014 },
  ];
  const segment = time / 1.15;
  const from = stops[Math.floor(segment) % stops.length];
  const to = stops[(Math.floor(segment) + 1) % stops.length];
  const raw = segment - Math.floor(segment);
  const blend = raw * raw * (3 - 2 * raw);
  return { x: from.x + (to.x - from.x) * blend, y: from.y + (to.y - from.y) * blend };
}

export function ObsessionField({ paused = false, phase, screen }: ObsessionSceneProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const reducedMotion = useMotionOff();
  const stateRef = useRef({ paused, phase, screen, reducedMotion });
  stateRef.current = { paused, phase, screen, reducedMotion };
  const [ready, setReady] = useState(false);
  const [fatalError, setFatalError] = useState<Error | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let disposed = false;
    let pipeline: ObsessionPipeline | null = null;
    let quality = obsessionQualityProfile("high");
    let width = 1;
    let height = 1;
    let phaseAge = 0;
    let previousPhase = stateRef.current.phase;
    let renderBudget = 1;
    let focus = obsessionFocusForScreen(stateRef.current.screen);
    let pointerX = 0;
    let pointerY = 0;

    const renderFrame = (dt: number, now = performance.now()) => {
      const state = stateRef.current;
      const still = state.reducedMotion || state.paused;
      const effectiveDt = still ? 0 : dt;
      renderBudget += dt;
      const interval = 1 / quality.targetFps;
      if (!still && renderBudget + 0.0001 < interval) return;
      const step = still ? 0 : renderBudget;
      renderBudget = 0;
      if (state.phase !== previousPhase) {
        previousPhase = state.phase;
        phaseAge = 0;
      } else {
        phaseAge += step;
      }
      const target = obsessionFocusForScreen(state.screen);
      const absoluteTime = now / 1000;
      const scan = state.phase === "scanning" && !still ? scanningOffset(absoluteTime) : { x: 0, y: 0 };
      focus = {
        x: damp(focus.x, target.x + scan.x, 2.4, Math.max(effectiveDt, 1 / 60)),
        y: damp(focus.y, target.y + scan.y, 2.4, Math.max(effectiveDt, 1 / 60)),
      };
      const motion = sampleObsessionEyeMotion({
        time: absoluteTime,
        phase: state.phase,
        phaseAge,
        pointerX,
        pointerY,
      });
      pipeline?.render(width, height, {
        time: absoluteTime,
        phase: obsessionPhaseValue(state.phase),
        phaseAge,
        focusX: focus.x,
        focusY: 1 - focus.y,
        pointerX,
        pointerY: -pointerY,
        capture: 1,
        gazeX: motion.gazeX,
        gazeY: -motion.gazeY,
        lidOpen: motion.lidOpen,
        pupilScale: motion.pupilScale,
        bodyTension: motion.bodyTension,
        irisRotation: motion.irisRotation,
        highlightPhase: motion.highlightPhase,
        fixation: motion.fixation,
        faultSplit: motion.faultSplit,
      });
    };

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      width = Math.max(1, Math.round((rect.width || window.innerWidth) * quality.resolutionScale * dpr));
      height = Math.max(1, Math.round((rect.height || window.innerHeight) * quality.resolutionScale * dpr));
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      loopRef.current?.invalidate();
    };

    const initialize = () => {
      try {
        pipeline = new ObsessionPipeline(canvas, quality);
        resize();
        renderFrame(0);
        if (!disposed) setReady(true);
      } catch (error) {
        if (!disposed) setFatalError(error instanceof Error ? error : new Error(String(error)));
      }
    };
    initialize();

    const loop = createRenderLoop(renderFrame, {
      role: "field",
      fps: 60,
      paused,
      onQualityChange: (tier) => {
        quality = obsessionQualityProfile(tier);
        pipeline?.setQuality(quality);
        resize();
      },
    });
    loopRef.current = loop;
    loop.start();

    const unsubscribePointer = subscribePointerFrame((pointer) => {
      pointerX = pointer.viewportX;
      pointerY = pointer.viewportY;
    });
    const onResize = () => resize();
    const onContextLost = (event: Event) => {
      event.preventDefault();
      pipeline?.abandonAfterContextLoss();
      pipeline = null;
      setReady(false);
    };
    const onContextRestored = () => {
      if (!disposed) initialize();
    };
    window.addEventListener("resize", onResize);
    canvas.addEventListener("webglcontextlost", onContextLost);
    canvas.addEventListener("webglcontextrestored", onContextRestored);

    return () => {
      disposed = true;
      loop.dispose();
      loopRef.current = null;
      unsubscribePointer();
      window.removeEventListener("resize", onResize);
      canvas.removeEventListener("webglcontextlost", onContextLost);
      canvas.removeEventListener("webglcontextrestored", onContextRestored);
      pipeline?.destroy();
      canvas.width = 0;
      canvas.height = 0;
    };
  }, []);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [paused, phase, screen, reducedMotion]);

  if (fatalError) throw fatalError;
  return (
    <div className="pointer-events-none absolute inset-0">
      {!ready && <ObsessionFallback paused={paused} phase={phase} screen={screen} />}
      <canvas
        ref={canvasRef}
        data-testid="obsession-webgl"
        className="absolute inset-0 h-full w-full transition-opacity duration-700"
        style={{ opacity: ready ? 1 : 0 }}
      />
    </div>
  );
}

export type { ObsessionVisualPhase };
