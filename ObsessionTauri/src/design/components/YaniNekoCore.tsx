import { useEffect, useRef } from "react";
import { motion } from "framer-motion";

import {
  CORE_HERO_MIN_SIZE,
  createRenderLoop,
  frameQualityScale,
  useRenderActive,
  type QualityTier,
  type RenderLoop,
} from "../render";
import { CoreShell } from "./CoreShell";
import { yaniCoreGeometry } from "./yanineko/geometry";
import { sampleYaniMotion, smoothYaniMotion } from "./yanineko/motion";
import { deriveYaniMood } from "./yanineko/state";
import type { YaniMood, YaniMotionFrame } from "./yanineko/types";

type Props = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  interactive?: boolean;
};

type HoverState = { target: number; x: number; y: number };

function createFaceGradient(ctx: CanvasRenderingContext2D, size: number, mood: YaniMood) {
  const gradient = ctx.createLinearGradient(size * 0.36, size * 0.3, size * 0.63, size * 0.82);
  gradient.addColorStop(0, mood === "alarm" ? "#f0c9b8" : "#f4d5c2");
  gradient.addColorStop(0.58, mood === "alarm" ? "#e3af9e" : "#edc3ac");
  gradient.addColorStop(1, "#a97768");
  return gradient;
}

function drawEar(
  ctx: CanvasRenderingContext2D,
  size: number,
  side: -1 | 1,
  attention: number,
  flat: number,
  glint: number,
) {
  const baseX = size * (side < 0 ? 0.27 : 0.73);
  const baseY = size * 0.31;
  const rotation = side * (0.04 + attention * 0.12 + flat * 0.58);
  ctx.save();
  ctx.translate(baseX, baseY + flat * size * 0.025);
  ctx.rotate(rotation);
  ctx.scale(side, 1);

  const hair = ctx.createLinearGradient(0, 0, 0, -size * 0.28);
  hair.addColorStop(0, "#819f90");
  hair.addColorStop(0.55, "#b5d4c3");
  hair.addColorStop(1, "#d9eadc");
  ctx.fillStyle = hair;
  ctx.strokeStyle = "rgba(25, 40, 34, 0.9)";
  ctx.lineWidth = size * 0.009;
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(-size * 0.115, size * 0.03);
  ctx.quadraticCurveTo(-size * 0.09, -size * 0.17, -size * 0.018, -size * (0.285 - flat * 0.035));
  ctx.quadraticCurveTo(size * 0.075, -size * 0.17, size * 0.13, size * 0.035);
  ctx.quadraticCurveTo(0, size * 0.07, -size * 0.115, size * 0.03);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  ctx.fillStyle = "rgba(207, 145, 136, 0.7)";
  ctx.beginPath();
  ctx.moveTo(-size * 0.064, -size * 0.005);
  ctx.quadraticCurveTo(-size * 0.05, -size * 0.14, -size * 0.017, -size * 0.22);
  ctx.quadraticCurveTo(size * 0.042, -size * 0.13, size * 0.075, 0);
  ctx.closePath();
  ctx.fill();

  if (side < 0) {
    ctx.lineWidth = size * 0.012;
    for (let index = 0; index < 2; index += 1) {
      const x = size * (0.075 + index * 0.045);
      const y = -size * (0.105 - index * 0.016);
      const radius = size * (0.026 - index * 0.003);
      ctx.strokeStyle = glint > 0.05 ? "#fff2bb" : "#d7ac53";
      ctx.shadowColor = `rgba(255, 224, 142, ${0.2 + glint * 0.7})`;
      ctx.shadowBlur = size * (0.012 + glint * 0.045);
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.shadowBlur = 0;
  }
  ctx.restore();
}

function drawEye(
  ctx: CanvasRenderingContext2D,
  size: number,
  x: number,
  y: number,
  mirror: boolean,
  frame: YaniMotionFrame,
  mood: YaniMood,
) {
  const open = Math.max(0.025, frame.eyeOpen);
  const halfWidth = size * 0.104;
  const halfHeight = size * (0.018 + open * 0.056);
  ctx.save();
  ctx.translate(x, y);
  if (mirror) ctx.scale(-1, 1);
  ctx.rotate(mood === "alarm" ? frame.grimace * 0.12 : mood === "scanning" ? -0.04 : 0);

  ctx.fillStyle = mood === "alarm" ? "rgba(250,245,232,0.88)" : "rgba(255,247,226,0.96)";
  ctx.strokeStyle = "rgba(31, 27, 23, 0.94)";
  ctx.lineWidth = size * (mood === "alarm" ? 0.008 : 0.011);
  ctx.beginPath();
  ctx.moveTo(-halfWidth, 0);
  ctx.bezierCurveTo(-halfWidth * 0.36, -halfHeight * 1.15, halfWidth * 0.42, -halfHeight, halfWidth, -halfHeight * 0.08);
  ctx.bezierCurveTo(halfWidth * 0.38, halfHeight * 0.92, -halfWidth * 0.38, halfHeight * 0.72, -halfWidth, 0);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  if (open > 0.08) {
    ctx.save();
    ctx.clip();
    const gazeX = frame.gazeX * size * 0.018;
    const gazeY = frame.gazeY * size * 0.012;
    const irisRadius = size * (mood === "scanning" ? 0.041 : 0.048);
    const iris = ctx.createRadialGradient(gazeX - irisRadius * 0.25, gazeY - irisRadius * 0.3, 0, gazeX, gazeY, irisRadius);
    iris.addColorStop(0, "#ffd56a");
    iris.addColorStop(0.52, "#e68b26");
    iris.addColorStop(1, "#8d3f12");
    ctx.fillStyle = iris;
    ctx.shadowColor = "rgba(232,137,46,0.7)";
    ctx.shadowBlur = size * 0.025;
    ctx.beginPath();
    ctx.arc(gazeX, gazeY, irisRadius, 0, Math.PI * 2);
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.fillStyle = "rgba(17,11,7,0.97)";
    ctx.beginPath();
    ctx.ellipse(gazeX, gazeY, size * (0.006 + frame.pupilScale * 0.013), irisRadius * 0.92, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "rgba(255,250,228,0.82)";
    ctx.beginPath();
    ctx.arc(gazeX - irisRadius * 0.28, gazeY - irisRadius * 0.34, size * 0.008, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  ctx.strokeStyle = "rgba(28,24,20,0.98)";
  ctx.lineWidth = size * 0.015;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(-halfWidth, 0);
  ctx.bezierCurveTo(-halfWidth * 0.32, -halfHeight * 1.18, halfWidth * 0.42, -halfHeight, halfWidth, -halfHeight * 0.08);
  ctx.stroke();
  ctx.restore();
}

function drawSmoke(ctx: CanvasRenderingContext2D, size: number, time: number, frame: YaniMotionFrame) {
  ctx.save();
  ctx.globalCompositeOperation = "screen";
  ctx.lineCap = "round";
  for (let strand = 0; strand < 3; strand += 1) {
    const phase = time * (0.32 + strand * 0.045) + strand * 1.9;
    const startX = size * 0.79;
    const startY = size * 0.725;
    const drift = Math.sin(phase) * size * 0.035;
    ctx.strokeStyle = `rgba(198, 218, 206, ${0.08 + frame.smoke * (0.08 - strand * 0.015)})`;
    ctx.lineWidth = size * (0.012 - strand * 0.002);
    ctx.shadowColor = "rgba(184, 218, 201, 0.28)";
    ctx.shadowBlur = size * 0.025;
    ctx.beginPath();
    ctx.moveTo(startX, startY);
    ctx.bezierCurveTo(
      startX + drift + strand * size * 0.018,
      startY - size * 0.11,
      startX - drift - size * 0.04,
      startY - size * 0.2,
      startX + Math.sin(phase * 0.7) * size * 0.06,
      startY - size * (0.3 + strand * 0.035),
    );
    ctx.stroke();
  }
  ctx.shadowBlur = 0;
  ctx.restore();
}

function drawYani(ctx: CanvasRenderingContext2D, size: number, time: number, frame: YaniMotionFrame, mood: YaniMood) {
  const geometry = yaniCoreGeometry(size);
  ctx.clearRect(0, 0, size, size);

  const halo = ctx.createRadialGradient(size * 0.5, size * 0.5, size * 0.08, size * 0.5, size * 0.5, size * 0.48);
  halo.addColorStop(0, `rgba(232,137,46,${0.08 + frame.ember * 0.13})`);
  halo.addColorStop(0.6, `rgba(126,174,151,${0.04 + frame.breath * 0.035})`);
  halo.addColorStop(1, "rgba(0,0,0,0)");
  ctx.fillStyle = halo;
  ctx.fillRect(0, 0, size, size);

  drawEar(ctx, size, -1, frame.leftEar, frame.earFlat, frame.ringGlint);
  drawEar(ctx, size, 1, frame.rightEar, frame.earFlat, 0);

  ctx.fillStyle = "rgba(18,17,14,0.78)";
  ctx.beginPath();
  ctx.moveTo(size * 0.37, size * 0.72);
  ctx.quadraticCurveTo(size * 0.5, size * 0.92, size * 0.66, size * 0.73);
  ctx.lineTo(size * 0.72, size * 0.9);
  ctx.lineTo(size * 0.28, size * 0.9);
  ctx.closePath();
  ctx.fill();

  ctx.save();
  ctx.shadowColor = mood === "active" ? "rgba(232,137,46,0.33)" : "rgba(107,157,136,0.22)";
  ctx.shadowBlur = size * 0.065;
  ctx.fillStyle = createFaceGradient(ctx, size, mood);
  ctx.strokeStyle = "rgba(28,34,29,0.92)";
  ctx.lineWidth = size * 0.01;
  ctx.beginPath();
  ctx.moveTo(size * 0.23, size * 0.34);
  ctx.bezierCurveTo(size * 0.12, size * 0.46, size * 0.14, size * 0.72, size * 0.34, size * 0.84);
  ctx.quadraticCurveTo(size * 0.5, size * 0.94, size * 0.67, size * 0.83);
  ctx.bezierCurveTo(size * 0.86, size * 0.68, size * 0.87, size * 0.44, size * 0.74, size * 0.31);
  ctx.quadraticCurveTo(size * 0.5, size * 0.2, size * 0.23, size * 0.34);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  ctx.restore();

  const hair = ctx.createLinearGradient(size * 0.25, size * 0.2, size * 0.72, size * 0.72);
  hair.addColorStop(0, "#d6eadc");
  hair.addColorStop(0.46, "#b9d7c6");
  hair.addColorStop(1, "#78988a");
  ctx.fillStyle = hair;
  ctx.strokeStyle = "rgba(30,45,38,0.82)";
  ctx.lineWidth = size * 0.009;
  ctx.beginPath();
  ctx.moveTo(size * 0.19, size * 0.43);
  ctx.quadraticCurveTo(size * 0.19, size * 0.2, size * 0.47, size * 0.18);
  ctx.quadraticCurveTo(size * 0.78, size * 0.17, size * 0.82, size * 0.42);
  ctx.lineTo(size * 0.77, size * 0.65);
  ctx.lineTo(size * 0.7, size * (0.57 + frame.hairLift * 0.025));
  ctx.lineTo(size * 0.67, size * 0.69);
  ctx.lineTo(size * 0.61, size * 0.55);
  ctx.lineTo(size * 0.57, size * 0.68);
  ctx.lineTo(size * 0.51, size * 0.5);
  ctx.lineTo(size * 0.44, size * 0.63);
  ctx.lineTo(size * 0.4, size * 0.47);
  ctx.lineTo(size * 0.31, size * 0.61);
  ctx.lineTo(size * 0.28, size * 0.49);
  ctx.lineTo(size * 0.2, size * 0.61);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  for (const side of [-1, 1]) {
    ctx.beginPath();
    if (side < 0) {
      ctx.moveTo(size * 0.2, size * 0.39);
      ctx.lineTo(size * 0.105, size * 0.62);
      ctx.lineTo(size * 0.2, size * 0.59);
      ctx.lineTo(size * 0.15, size * 0.75);
      ctx.lineTo(size * 0.28, size * 0.67);
    } else {
      ctx.moveTo(size * 0.79, size * 0.38);
      ctx.lineTo(size * 0.9, size * 0.59);
      ctx.lineTo(size * 0.81, size * 0.58);
      ctx.lineTo(size * 0.86, size * 0.72);
      ctx.lineTo(size * 0.72, size * 0.66);
    }
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
  }

  ctx.strokeStyle = "rgba(244,255,247,0.52)";
  ctx.lineWidth = size * 0.007;
  ctx.lineCap = "round";
  for (let index = 0; index < 5; index += 1) {
    const x = size * (0.34 + index * 0.075);
    ctx.beginPath();
    ctx.moveTo(x, size * (0.245 + (index % 2) * 0.012));
    ctx.lineTo(x - size * 0.012, size * (0.31 + frame.hairLift * 0.01));
    ctx.stroke();
  }

  drawEye(ctx, size, geometry.leftEye.x, geometry.leftEye.y, false, frame, mood);
  drawEye(ctx, size, geometry.rightEye.x, geometry.rightEye.y, true, frame, mood);

  ctx.strokeStyle = "rgba(43,34,28,0.7)";
  ctx.lineWidth = size * 0.007;
  ctx.beginPath();
  ctx.moveTo(size * 0.29, size * (0.405 + frame.grimace * 0.025));
  ctx.quadraticCurveTo(size * 0.36, size * (0.38 - frame.grimace * 0.045), size * 0.43, size * 0.405);
  ctx.moveTo(size * 0.57, size * 0.405);
  ctx.quadraticCurveTo(size * 0.65, size * (0.38 - frame.grimace * 0.045), size * 0.72, size * 0.405);
  ctx.stroke();

  ctx.strokeStyle = `rgba(197, 75, 70, ${0.08 + frame.blush * 0.5})`;
  ctx.lineWidth = size * 0.006;
  for (const side of [-1, 1]) {
    for (let index = 0; index < 4; index += 1) {
      const x = size * (0.5 + side * (0.205 + index * 0.018));
      ctx.beginPath();
      ctx.moveTo(x, size * 0.605);
      ctx.lineTo(x + size * 0.016, size * 0.575);
      ctx.stroke();
    }
  }

  if (mood === "alarm") {
    const faceSqueeze = frame.grimace;
    ctx.fillStyle = `rgba(196, 86, 78, ${0.08 + frame.blush * 0.13})`;
    ctx.beginPath();
    ctx.ellipse(size * 0.39, size * 0.63, size * 0.09, size * 0.06, -0.16, 0, Math.PI * 2);
    ctx.ellipse(size * 0.61, size * 0.63, size * 0.09, size * 0.06, 0.16, 0, Math.PI * 2);
    ctx.fill();

    // The swollen, folded cat-mouth is the deliberately grotesque side of
    // Yaniko: readable at 104 px, but reserved for a real fault signal.
    ctx.strokeStyle = "rgba(54,34,29,0.94)";
    ctx.lineWidth = size * 0.012;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(size * 0.5, size * 0.585);
    ctx.bezierCurveTo(size * 0.475, size * 0.61, size * 0.43, size * 0.59, size * 0.405, size * (0.655 + faceSqueeze * 0.012));
    ctx.bezierCurveTo(size * 0.425, size * 0.705, size * 0.48, size * 0.69, size * 0.505, size * 0.665);
    ctx.bezierCurveTo(size * 0.535, size * 0.705, size * 0.59, size * 0.69, size * 0.615, size * (0.64 - faceSqueeze * 0.01));
    ctx.bezierCurveTo(size * 0.58, size * 0.59, size * 0.53, size * 0.61, size * 0.5, size * 0.585);
    ctx.stroke();

    ctx.fillStyle = "rgba(93,42,36,0.92)";
    ctx.beginPath();
    ctx.ellipse(size * 0.51, size * 0.704, size * 0.055, size * 0.034, -0.04, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "rgba(226,121,116,0.88)";
    ctx.beginPath();
    ctx.ellipse(size * 0.515, size * 0.716, size * 0.034, size * 0.017, -0.04, 0, Math.PI);
    ctx.fill();
  } else {
    ctx.strokeStyle = "rgba(83,55,46,0.64)";
    ctx.lineWidth = size * 0.006;
    ctx.beginPath();
    ctx.moveTo(size * 0.5, size * 0.565);
    ctx.quadraticCurveTo(size * 0.485, size * 0.625, size * 0.515, size * 0.635);
    ctx.stroke();
    ctx.strokeStyle = "rgba(47,32,27,0.9)";
    ctx.lineWidth = size * 0.009;
    ctx.beginPath();
    ctx.moveTo(size * 0.43, size * 0.66);
    ctx.quadraticCurveTo(size * (0.47 - frame.grimace * 0.02), size * (0.69 + frame.grimace * 0.03), size * 0.51, size * 0.66);
    ctx.quadraticCurveTo(size * (0.55 + frame.grimace * 0.035), size * (0.69 - frame.grimace * 0.015), size * 0.61, size * 0.655);
    ctx.stroke();
  }

  const tremor = Math.sin(time * 29) * frame.cigaretteTremor * size * 0.004;
  const startX = size * 0.54;
  const startY = size * (mood === "alarm" ? 0.742 : 0.675);
  const endX = geometry.cigaretteEnd.x;
  const endY = geometry.cigaretteEnd.y + tremor + frame.grimace * size * 0.02;
  const angle = Math.atan2(endY - startY, endX - startX);
  const length = Math.hypot(endX - startX, endY - startY);
  ctx.save();
  ctx.translate(startX, startY);
  ctx.rotate(angle);
  ctx.fillStyle = "#f4eddc";
  ctx.strokeStyle = "rgba(58,48,38,0.72)";
  ctx.lineWidth = size * 0.004;
  ctx.beginPath();
  ctx.roundRect(0, -size * 0.018, length, size * 0.036, size * 0.012);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = "rgba(67,94,153,0.9)";
  ctx.fillRect(length * 0.12, -size * 0.017, size * 0.018, size * 0.034);
  ctx.fillStyle = "rgba(144,139,128,0.9)";
  ctx.fillRect(length - size * 0.026, -size * 0.017, size * 0.026, size * 0.034);
  ctx.restore();

  const emberX = endX;
  const emberY = endY;
  const emberRadius = size * (0.015 + frame.ember * 0.008);
  const ember = ctx.createRadialGradient(emberX, emberY, 0, emberX, emberY, emberRadius * 4.5);
  ember.addColorStop(0, "rgba(255,244,205,1)");
  ember.addColorStop(0.18, `rgba(255,164,56,${0.5 + frame.ember * 0.5})`);
  ember.addColorStop(0.5, `rgba(220,70,20,${0.25 + frame.ember * 0.45})`);
  ember.addColorStop(1, "rgba(220,70,20,0)");
  ctx.fillStyle = ember;
  ctx.fillRect(emberX - emberRadius * 5, emberY - emberRadius * 5, emberRadius * 10, emberRadius * 10);

  drawSmoke(ctx, size, time, frame);
}

function YaniCanvas({ mood, size, paused, interactive }: { mood: YaniMood; size: number; paused: boolean; interactive: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef({ mood });
  stateRef.current = { mood };
  const loopRef = useRef<RenderLoop | null>(null);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  const hoverRef = useRef<HoverState>({ target: 0, x: 0, y: 0 });

  useEffect(() => {
    if (!interactive) return;
    const host = canvasRef.current?.parentElement;
    if (!host) return;
    const onMove = (event: PointerEvent) => {
      const rect = host.getBoundingClientRect();
      hoverRef.current = {
        target: 1,
        x: ((event.clientX - rect.left) / rect.width - 0.5) * 2,
        y: ((event.clientY - rect.top) / rect.height - 0.5) * 2,
      };
    };
    const onLeave = () => { hoverRef.current.target = 0; };
    host.addEventListener("pointermove", onMove);
    host.addEventListener("pointerleave", onLeave);
    return () => {
      host.removeEventListener("pointermove", onMove);
      host.removeEventListener("pointerleave", onLeave);
    };
  }, [interactive]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const role = size >= CORE_HERO_MIN_SIZE ? "hero" : "preview";
    const baseDpr = role === "hero" ? Math.min(window.devicePixelRatio || 1, 2) : 1;
    let backingDpr = 0;
    const resizeBacking = (tier: QualityTier) => {
      const quality = frameQualityScale(tier);
      const nextDpr = role === "hero" ? Math.max(1, baseDpr * quality) : Math.max(0.65, quality);
      if (Math.abs(backingDpr - nextDpr) < 0.001) return;
      backingDpr = nextDpr;
      canvas.width = Math.max(1, Math.round(size * backingDpr));
      canvas.height = Math.max(1, Math.round(size * backingDpr));
      ctx.setTransform(backingDpr, 0, 0, backingDpr, 0, 0);
    };
    resizeBacking("high");

    let time = 0;
    let moodAge = 0;
    let previousMood = stateRef.current.mood;
    let hover = 0;
    let displayed = sampleYaniMotion({ time, mood: previousMood, moodAge: 0 });
    const draw = (dt: number) => {
      const nextMood = stateRef.current.mood;
      const effectiveDt = pausedRef.current ? 0 : dt;
      time += effectiveDt;
      if (nextMood !== previousMood) { previousMood = nextMood; moodAge = 0; }
      else moodAge += effectiveDt;
      hover += (hoverRef.current.target - hover) * (1 - Math.exp(-dt * 8));
      const target = sampleYaniMotion({
        time,
        mood: nextMood,
        moodAge,
        pointerX: hoverRef.current.x,
        pointerY: hoverRef.current.y,
        hover,
      });
      displayed = pausedRef.current ? target : smoothYaniMotion(displayed, target, dt, nextMood === "alarm" ? 14 : 9);
      drawYani(ctx, size, time, displayed, nextMood);
    };
    const loop = createRenderLoop(draw, { role, paused: pausedRef.current, onQualityChange: resizeBacking });
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
  }, [mood, paused]);

  return <canvas ref={canvasRef} aria-hidden="true" data-yani-core data-mood={mood} data-motion={paused ? "still" : "running"} className="pointer-events-none absolute" style={{ width: size, height: size }} />;
}

export function YaniNekoCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  onClick,
  size = 240,
  paused = false,
  interactive = true,
}: Props) {
  const mood = deriveYaniMood({ active, busy, scanning, alarm });
  const renderOn = useRenderActive() && !paused;
  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <motion.div
        aria-hidden
        className="pointer-events-none absolute"
        style={{
          inset: size * 0.02,
          clipPath: "polygon(7% 42%, 17% 5%, 34% 24%, 50% 16%, 68% 23%, 84% 3%, 94% 44%, 86% 79%, 66% 96%, 34% 96%, 12% 78%)",
          background: mood === "alarm"
            ? "radial-gradient(circle at 50% 58%, rgba(196,70,58,.2), rgba(74,109,88,.06) 58%, transparent 76%)"
            : "radial-gradient(circle at 50% 58%, rgba(232,137,46,.16), rgba(157,207,181,.08) 58%, transparent 78%)",
          filter: `blur(${Math.max(5, size * 0.035)}px)`,
        }}
        animate={renderOn ? { opacity: [0.62, 0.95, 0.62], scale: [0.99, 1.018, 0.99] } : { opacity: 0.72, scale: 1 }}
        transition={renderOn ? { duration: mood === "alarm" ? 1.1 : 4.8, repeat: Infinity, ease: "easeInOut" } : { duration: 0 }}
      />
      <YaniCanvas mood={mood} size={size} paused={paused} interactive={interactive} />
    </CoreShell>
  );
}
