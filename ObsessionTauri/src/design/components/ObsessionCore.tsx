import { useEffect, useRef } from "react";

import {
  CORE_HERO_MIN_SIZE,
  createRenderLoop,
  frameQualityScale,
  type QualityTier,
  type RenderLoop,
} from "../render";
import { subscribePointerFrame } from "../pointerBus";
import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { CoreShell } from "./CoreShell";
import { drawObsessionEye } from "./obsession/eyeCanvas";
import { sampleObsessionEyeMotion } from "./obsession/eyeMotion";

type Props = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  paused?: boolean;
  interactive?: boolean;
  onClick: () => void;
  size?: number;
};

function phaseFor(active: boolean, busy: boolean, scanning: boolean, alarm: boolean): ObsessionVisualPhase {
  return alarm ? "fault" : scanning ? "scanning" : busy ? "engaging" : active ? "focused" : "idle";
}

export function ObsessionCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  paused = false,
  interactive = true,
  onClick,
  size = 240,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const phase = phaseFor(active, busy, scanning, alarm);
  const stateRef = useRef({ active, phase, paused });
  stateRef.current = { active, phase, paused };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const role = size >= CORE_HERO_MIN_SIZE ? "hero" : "preview";
    const baseDpr = role === "hero" ? Math.min(window.devicePixelRatio || 1, 2) : 1;
    let backingDpr = 0;
    let previousPhase = stateRef.current.phase;
    let phaseStartedAt = performance.now();
    let pointerX = 0;
    let pointerY = 0;

    const resizeBacking = (tier: QualityTier) => {
      const qualityScale = frameQualityScale(tier);
      const dpr = role === "hero"
        ? Math.max(1, baseDpr * qualityScale)
        : Math.max(0.65, qualityScale);
      if (Math.abs(dpr - backingDpr) < 0.001) return;
      backingDpr = dpr;
      canvas.width = Math.max(1, Math.round(size * dpr));
      canvas.height = Math.max(1, Math.round(size * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resizeBacking("high");

    const draw = (_dt: number, now: number) => {
      const state = stateRef.current;
      if (state.phase !== previousPhase) {
        previousPhase = state.phase;
        phaseStartedAt = now;
      }
      const motion = sampleObsessionEyeMotion({
        time: now / 1000,
        phase: state.phase,
        phaseAge: Math.max(0, now - phaseStartedAt) / 1000,
        pointerX,
        pointerY,
      });
      drawObsessionEye(ctx, size, motion, { phase: state.phase, active: state.active });
    };

    const loop = createRenderLoop(draw, {
      role,
      fps: role === "preview" ? 30 : undefined,
      paused: stateRef.current.paused,
      onQualityChange: resizeBacking,
    });
    loopRef.current = loop;
    loop.start();
    const unsubscribePointer = subscribePointerFrame((pointer) => {
      pointerX = pointer.viewportX;
      pointerY = pointer.viewportY;
    });
    return () => {
      unsubscribePointer();
      loop.dispose();
      loopRef.current = null;
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [size]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [active, phase, paused]);

  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        data-obsession-core
        data-phase={phase}
        data-motion={paused ? "still" : "running"}
        className="pointer-events-none absolute inset-0"
        style={{ width: size, height: size }}
      />
    </CoreShell>
  );
}
