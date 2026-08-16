import { useEffect, useRef } from "react";

import {
  CORE_HERO_MIN_SIZE,
  createRenderLoop,
  frameQualityScale,
  useMotionOff,
  type QualityTier,
  type RenderLoop,
} from "../render";
import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { CoreShell } from "./CoreShell";
import { sampleObsessionChoirMotion, smoothObsessionChoirMotion } from "./obsessionChoir/motion";
import {
  drawObsessionChoirSeal,
  obsessionChoirSealSpinSpeed,
} from "./obsessionChoir/sealCanvas";

type Props = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  paused?: boolean;
  interactive?: boolean;
  forceRitual?: boolean;
  onClick: () => void;
  size?: number;
};

function phaseFor(active: boolean, busy: boolean, scanning: boolean, alarm: boolean): ObsessionVisualPhase {
  return alarm ? "fault" : scanning ? "scanning" : busy ? "engaging" : active ? "focused" : "idle";
}

export function ObsessionChoirCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  paused = false,
  interactive = true,
  forceRitual = false,
  onClick,
  size = 240,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const motionOff = useMotionOff();
  const phase = phaseFor(active, busy, scanning, alarm);
  const stateRef = useRef({ active, paused, motionOff, phase, forceRitual });
  stateRef.current = { active, paused, motionOff, phase, forceRitual };

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const role = size >= CORE_HERO_MIN_SIZE ? "hero" : "preview";
    const baseDpr = role === "hero" ? Math.min(window.devicePixelRatio || 1, 2) : 1;
    let dpr = 0;
    let previousPhase = stateRef.current.phase;
    let phaseStartedAt = performance.now();
    let displayedMotion = sampleObsessionChoirMotion({
      time: phaseStartedAt / 1000,
      phase: previousPhase,
      phaseAge: 0,
      forceRitual: stateRef.current.forceRitual,
    });
    let displayedActivity = stateRef.current.active ? 1 : 0;
    let sealSpin = 0;

    const resize = (tier: QualityTier) => {
      const nextDpr = role === "hero"
        ? Math.max(1, baseDpr * frameQualityScale(tier))
        : Math.max(0.7, frameQualityScale(tier));
      if (Math.abs(dpr - nextDpr) < 0.001) return;
      dpr = nextDpr;
      canvas.width = Math.round(size * dpr);
      canvas.height = Math.round(size * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize("high");

    const loop = createRenderLoop((dt, now) => {
      const state = stateRef.current;
      if (state.phase !== previousPhase) {
        previousPhase = state.phase;
        phaseStartedAt = now;
      }
      const targetMotion = sampleObsessionChoirMotion({
        time: now / 1000,
        phase: state.phase,
        phaseAge: Math.max(0, now - phaseStartedAt) / 1000,
        forceRitual: state.forceRitual,
      });
      const stillFrame = state.paused || state.motionOff;
      displayedMotion = stillFrame
        ? targetMotion
        : smoothObsessionChoirMotion(
          displayedMotion,
          targetMotion,
          dt,
          state.phase === "fault" ? 10.5 : 7.25,
        );
      if (stillFrame) {
        displayedActivity = state.active ? 1 : 0;
      } else {
        const activityAlpha = 1 - Math.exp(-Math.max(0, Math.min(dt, 0.25)) * 5.5);
        displayedActivity += ((state.active ? 1 : 0) - displayedActivity) * activityAlpha;
        sealSpin = (
          sealSpin
          + obsessionChoirSealSpinSpeed(displayedActivity) * Math.max(0, Math.min(dt, 0.1))
        ) % (Math.PI * 2);
      }
      drawObsessionChoirSeal(ctx, size, displayedMotion, displayedActivity, sealSpin);
    }, {
      role,
      fps: role === "preview" ? 30 : undefined,
      paused: stateRef.current.paused,
      onQualityChange: resize,
    });
    loopRef.current = loop;
    loop.start();
    return () => {
      loop.dispose();
      loopRef.current = null;
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [size]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [active, forceRitual, motionOff, paused, phase]);

  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        data-choir-seal
        data-phase={phase}
        data-motion={paused ? "still" : "running"}
        className="pointer-events-none absolute inset-0"
        style={{ width: size, height: size }}
      />
    </CoreShell>
  );
}
