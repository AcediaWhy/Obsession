import { useEffect, useRef } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import {
  drawRoofCatSprite,
  type RoofCatDetail as ResolvedSunkenStarDetail,
} from "./rooftopCatSprite";
import "./SunkenStarSpriteLab.css";

export type SunkenStarDetail = "auto" | ResolvedSunkenStarDetail;

export type SunkenStarSpriteLabProps = {
  phase?: ObsessionVisualPhase;
  detail?: SunkenStarDetail;
  paused?: boolean;
  size?: number | string;
};

type Mood = "idle" | "busy" | "scanning" | "active" | "alarm";

export function sunkenStarMoodForPhase(phase: ObsessionVisualPhase): Mood {
  switch (phase) {
    case "engaging": return "busy";
    case "scanning": return "scanning";
    case "focused": return "active";
    case "fault": return "alarm";
    case "idle":
    default: return "idle";
  }
}

function resolveDetail(detail: SunkenStarDetail, size?: number | string): ResolvedSunkenStarDetail {
  if (detail === "base" || detail === "hero") return detail;
  const numeric = typeof size === "number" ? size : Number.parseFloat(size || "240");
  return Number.isFinite(numeric) && numeric <= 120 ? "base" : "hero";
}

export function SunkenStarSpriteLab({
  phase = "idle",
  detail = "auto",
  paused = false,
  size = 240,
}: SunkenStarSpriteLabProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const mood = sunkenStarMoodForPhase(phase);
  const resolvedDetail = resolveDetail(detail, size);
  const motion = paused ? "paused" : "running";
  const sizeValue = typeof size === "number" ? `${size}px` : size;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return undefined;
    const context = canvas.getContext("2d");
    if (!context) return undefined;

    canvas.width = 64;
    canvas.height = 64;
    context.imageSmoothingEnabled = false;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const motionPaused = paused || reduceMotion;
    let animationFrame = 0;
    let lastFrame = -1;

    const tick = (time: number) => {
      const frame = motionPaused ? 0 : Math.floor(time / 90);
      if (frame !== lastFrame) {
        drawRoofCatSprite(context, phase, frame, resolvedDetail);
        lastFrame = frame;
      }
      if (!motionPaused) animationFrame = window.requestAnimationFrame(tick);
    };

    tick(0);
    return () => window.cancelAnimationFrame(animationFrame);
  }, [paused, phase, resolvedDetail]);

  return (
    <div
      aria-label="Процедурно нарисованный кот на крыше у миски с молоком"
      className="sunken-star-sprite-lab"
      data-detail={resolvedDetail}
      data-mood={mood}
      data-motion={motion}
      data-phase={phase}
      data-renderer="procedural-canvas"
      data-sunken-star-sprite-lab
      role="img"
      style={{ ["--sunken-star-size" as string]: sizeValue }}
    >
      <canvas aria-hidden="true" className="sunken-star-sprite-lab__canvas" ref={canvasRef} />
    </div>
  );
}
