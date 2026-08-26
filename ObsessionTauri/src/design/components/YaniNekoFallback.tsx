import { useEffect, useMemo, useRef } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { createRenderLoop, frameQualityScale, type QualityTier, type RenderLoop } from "../render";
import { yaniHash } from "./yanineko/random";
import { yaniMoodForPhase } from "./yanineko/state";

type Props = {
  phase?: ObsessionVisualPhase;
  screen?: string;
  paused?: boolean;
  qualityTier?: QualityTier;
};

type Dust = { x: number; y: number; radius: number; drift: number; phase: number };

function screenShift(screen: string): { x: number; y: number } {
  if (screen === "settings") return { x: -0.018, y: 0 };
  if (screen === "dpi") return { x: 0.012, y: 0.006 };
  if (screen === "telegram") return { x: -0.01, y: -0.004 };
  return { x: 0, y: 0 };
}
function drawFallbackRoom(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  phase: ObsessionVisualPhase,
  screen: string,
  dust: readonly Dust[],
) {
  const mood = yaniMoodForPhase(phase);
  const hot = mood === "active" ? 1 : mood === "busy" ? 0.7 : 0;
  const alarm = mood === "alarm" ? 1 : 0;
  const shift = screenShift(screen);
  const sx = shift.x * width;
  const sy = shift.y * height;
  ctx.clearRect(0, 0, width, height);

  const wall = ctx.createLinearGradient(0, 0, 0, height);
  wall.addColorStop(0, "#080a08");
  wall.addColorStop(0.58, "#100d09");
  wall.addColorStop(1, "#1a1009");
  ctx.fillStyle = wall;
  ctx.fillRect(0, 0, width, height);

  // Window and blinds.
  const wx = width * 0.69 + sx;
  const wy = height * 0.08 + sy;
  const ww = width * 0.27;
  const wh = height * 0.48;
  const windowLight = ctx.createLinearGradient(wx, wy, wx, wy + wh);
  windowLight.addColorStop(0, "rgba(124,174,151,.54)");
  windowLight.addColorStop(1, "rgba(45,79,66,.22)");
  ctx.fillStyle = windowLight;
  ctx.fillRect(wx, wy, ww, wh);
  ctx.strokeStyle = "rgba(24,35,29,.9)";
  ctx.lineWidth = Math.max(4, width * 0.009);
  ctx.strokeRect(wx, wy, ww, wh);
  ctx.beginPath();
  ctx.moveTo(wx + ww * 0.5, wy);
  ctx.lineTo(wx + ww * 0.5, wy + wh);
  ctx.moveTo(wx, wy + wh * 0.5);
  ctx.lineTo(wx + ww, wy + wh * 0.5);
  ctx.stroke();
  ctx.lineWidth = Math.max(1, height * 0.004);
  for (let index = 1; index < 9; index += 1) {
    const y = wy + (wh / 9) * index;
    ctx.strokeStyle = "rgba(27,42,35,.42)";
    ctx.beginPath();
    ctx.moveTo(wx, y);
    ctx.lineTo(wx + ww, y);
    ctx.stroke();
  }

  // Soft beam trapezoid.
  ctx.save();
  ctx.globalCompositeOperation = "screen";
  const beam = ctx.createLinearGradient(wx + ww * 0.55, wy, width * 0.35, height);
  beam.addColorStop(0, `rgba(126,184,158,${0.11 + hot * 0.03})`);
  beam.addColorStop(1, "rgba(126,184,158,0)");
  ctx.fillStyle = beam;
  ctx.beginPath();
  ctx.moveTo(wx + ww * 0.32, wy + wh * 0.55);
  ctx.lineTo(wx + ww * 0.92, wy + wh * 0.55);
  ctx.lineTo(width * 0.58, height);
  ctx.lineTo(width * 0.17, height);
  ctx.closePath();
  ctx.fill();
  ctx.restore();

  // Desk.
  const deskY = height * 0.75;
  const desk = ctx.createLinearGradient(0, deskY, 0, height);
  desk.addColorStop(0, "#382114");
  desk.addColorStop(1, "#120b07");
  ctx.fillStyle = desk;
  ctx.fillRect(0, deskY, width, height - deskY);
  ctx.fillStyle = "rgba(231,142,52,.12)";
  ctx.fillRect(0, deskY, width, Math.max(1, height * 0.005));

  // Ashtray.
  const ax = width * 0.315 + sx;
  const ay = height * 0.855 + sy;
  ctx.fillStyle = "rgba(154,158,148,.72)";
  ctx.beginPath();
  ctx.ellipse(ax, ay, width * 0.092, height * 0.027, 0, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "rgba(20,18,15,.9)";
  ctx.beginPath();
  ctx.ellipse(ax, ay - height * 0.004, width * 0.068, height * 0.016, 0, 0, Math.PI * 2);
  ctx.fill();

  // Pack with the signature blue stripe.
  const px = width * 0.55 + sx;
  const py = height * 0.79 + sy;
  ctx.save();
  ctx.rotate(-0.055);
  ctx.fillStyle = "rgba(190,218,202,.68)";
  ctx.fillRect(px, py, width * 0.08, height * 0.13);
  ctx.fillStyle = "rgba(53,76,129,.9)";
  ctx.fillRect(px, py + height * 0.04, width * 0.08, height * 0.016);
  ctx.restore();

  // Can and crumpled fabric complete the cinematic clutter.
  const canX = width * 0.76 + sx;
  ctx.fillStyle = "rgba(74,91,81,.6)";
  ctx.fillRect(canX, height * 0.73 + sy, width * 0.055, height * 0.2);
  ctx.fillStyle = "rgba(186,211,198,.18)";
  ctx.fillRect(canX + width * 0.008, height * 0.74 + sy, width * 0.008, height * 0.17);
  ctx.fillStyle = "rgba(76,91,82,.38)";
  ctx.beginPath();
  ctx.ellipse(width * 0.44 + sx, height * 0.91 + sy, width * 0.16, height * 0.055, -0.08, 0, Math.PI * 2);
  ctx.fill();

  // Smoke is drawn as long coherent curves rather than soft random blobs.
  ctx.save();
  ctx.globalCompositeOperation = "screen";
  ctx.lineCap = "round";
  for (let strand = 0; strand < 7; strand += 1) {
    const seed = yaniHash(strand * 71 + 9);
    const phaseOffset = time * (0.12 + seed * 0.07) + seed * 9;
    const sourceX = ax + (seed - 0.5) * width * 0.055;
    const sourceY = ay - height * 0.025;
    const rise = height * (0.32 + seed * 0.22);
    ctx.strokeStyle = `rgba(188,208,196,${0.035 + (hot + 0.4) * 0.025})`;
    ctx.lineWidth = width * (0.008 + seed * 0.006);
    ctx.shadowColor = "rgba(166,206,187,.18)";
    ctx.shadowBlur = width * 0.018;
    ctx.beginPath();
    ctx.moveTo(sourceX, sourceY);
    ctx.bezierCurveTo(
      sourceX + Math.sin(phaseOffset) * width * 0.06,
      sourceY - rise * 0.3,
      sourceX - Math.sin(phaseOffset * 0.63) * width * 0.1,
      sourceY - rise * 0.67,
      sourceX + Math.sin(phaseOffset * 0.41) * width * 0.13,
      sourceY - rise,
    );
    ctx.stroke();
  }
  ctx.shadowBlur = 0;
  ctx.restore();

  const emberPulse = 0.55 + hot * 0.4 + Math.max(0, Math.sin(time * 1.38)) * 0.15;
  const ember = ctx.createRadialGradient(ax, ay - height * 0.016, 0, ax, ay - height * 0.016, width * 0.035);
  ember.addColorStop(0, `rgba(255,231,173,${(1 - alarm) * emberPulse})`);
  ember.addColorStop(0.25, `rgba(243,126,36,${(1 - alarm) * emberPulse * 0.8})`);
  ember.addColorStop(1, "rgba(220,65,18,0)");
  ctx.fillStyle = ember;
  ctx.fillRect(ax - width * 0.04, ay - height * 0.06, width * 0.08, height * 0.08);

  ctx.save();
  ctx.globalCompositeOperation = "screen";
  for (const mote of dust) {
    const x = (mote.x + Math.sin(time * mote.drift + mote.phase) * 0.025) * width;
    const y = ((mote.y - time * 0.008 * mote.drift) % 1 + 1) % 1 * height;
    ctx.fillStyle = `rgba(201,229,213,${0.08 + hot * 0.025})`;
    ctx.beginPath();
    ctx.arc(x, y, mote.radius * width, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();

  if (alarm) {
    ctx.fillStyle = `rgba(94,147,117,${0.035 + 0.03 * Math.abs(Math.sin(time * 19))})`;
    ctx.fillRect(0, 0, width, height);
  }
  const vignette = ctx.createRadialGradient(width * 0.5, height * 0.5, height * 0.15, width * 0.5, height * 0.5, width * 0.65);
  vignette.addColorStop(0, "rgba(0,0,0,0)");
  vignette.addColorStop(1, "rgba(0,0,0,.58)");
  ctx.fillStyle = vignette;
  ctx.fillRect(0, 0, width, height);
}

export function YaniNekoFallback({ phase = "idle", screen = "overview", paused = false, qualityTier }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const stateRef = useRef({ phase, screen, paused, qualityTier });
  stateRef.current = { phase, screen, paused, qualityTier };
  const dust = useMemo<Dust[]>(() => Array.from({ length: 44 }, (_, index) => ({
    x: yaniHash(index * 13 + 3),
    y: yaniHash(index * 17 + 5),
    radius: 0.0007 + yaniHash(index * 19 + 7) * 0.0015,
    drift: 0.4 + yaniHash(index * 23 + 11) * 0.8,
    phase: yaniHash(index * 29 + 13) * Math.PI * 2,
  })), []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let width = 1;
    let height = 1;
    let backingScale = 0.75 * frameQualityScale(stateRef.current.qualityTier ?? "high");
    const resize = () => {
      width = Math.max(1, canvas.clientWidth);
      height = Math.max(1, canvas.clientHeight);
      canvas.width = Math.max(1, Math.round(width * backingScale));
      canvas.height = Math.max(1, Math.round(height * backingScale));
      ctx.setTransform(backingScale, 0, 0, backingScale, 0, 0);
    };
    resize();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      resize();
      loopRef.current?.invalidate();
    });
    observer?.observe(canvas);
    let time = 0;
    const draw = (dt: number) => {
      if (!stateRef.current.paused) time += dt;
      drawFallbackRoom(ctx, width, height, time, stateRef.current.phase, stateRef.current.screen, dust);
    };
    const loop = createRenderLoop(draw, {
      role: "field",
      paused: stateRef.current.paused,
      onQualityChange: (tier) => {
        backingScale = 0.75 * frameQualityScale(stateRef.current.qualityTier ?? tier);
        resize();
      },
    });
    loopRef.current = loop;
    loop.start();
    return () => {
      observer?.disconnect();
      loop.dispose();
      loopRef.current = null;
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [dust]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [paused, phase, qualityTier, screen]);

  return <canvas ref={canvasRef} aria-hidden="true" data-yani-fallback className="absolute inset-0 h-full w-full" />;
}
