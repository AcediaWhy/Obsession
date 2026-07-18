import { useEffect, useRef } from "react";
import { motion } from "framer-motion";

import {
  CORE_HERO_MIN_SIZE,
  createRenderLoop,
  frameQualityScale,
  useMotionOff,
  useRenderHidden,
  type QualityTier,
  type RenderLoop,
} from "../render";
import { RainCoreModel, type RainCoreSnapshot } from "./rain/coreModel";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  variant?: "control" | "preview";
};

type GlassDrop = { x: number; y: number; radius: number; speed: number; alpha: number };

export function RainLanternCore({
  active,
  busy = false,
  onClick,
  size = 240,
  paused = false,
  variant,
}: Props) {
  const hidden = useRenderHidden();
  const motionOff = useMotionOff();
  const resolvedVariant = variant ?? (size >= CORE_HERO_MIN_SIZE ? "control" : "preview");
  if (hidden) return <div style={{ width: size, height: size }} />;

  const content = (
    <>
      <div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.01,
          background: active
            ? "radial-gradient(circle, rgba(222,132,73,0.17), rgba(81,111,132,0.08) 48%, transparent 72%)"
            : "radial-gradient(circle, rgba(89,111,126,0.12), transparent 70%)",
          filter: `blur(${Math.max(8, size * 0.05)}px)`,
        }}
      />
      <LanternCanvas
        active={active}
        busy={busy}
        size={size}
        paused={paused}
        reducedMotion={motionOff}
        variant={resolvedVariant}
      />
      <div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.055,
          border: "1px solid rgba(222,235,242,0.12)",
          boxShadow: active
            ? "inset 0 0 30px rgba(238,162,104,0.10), 0 0 18px rgba(211,130,75,0.13)"
            : "inset 0 0 30px rgba(148,176,193,0.08), 0 0 12px rgba(91,118,137,0.10)",
        }}
      />
      <span
        className="pointer-events-none absolute rounded-full px-2 py-1 text-2xs font-bold tracking-[0.32em]"
        style={{
          bottom: size * 0.21,
          color: active ? "#F5D0B5" : "#C1D0DA",
          background: "rgba(3,7,11,0.24)",
          textShadow: active ? "0 0 12px rgba(225,139,82,0.58)" : "0 0 10px rgba(130,158,177,0.48)",
        }}
      >
        {busy ? "···" : active ? "ON" : "OFF"}
      </span>
    </>
  );

  if (resolvedVariant === "preview") {
    return (
      <div aria-hidden="true" className="relative flex items-center justify-center" style={{ width: size, height: size }}>
        {content}
      </div>
    );
  }
  return (
    <motion.button
      type="button"
      aria-label={busy ? "Изменение состояния" : active ? "Выключить" : "Включить"}
      aria-pressed={active}
      aria-busy={busy}
      onClick={onClick}
      disabled={busy}
      whileHover={motionOff ? undefined : { scale: 1.025 }}
      whileTap={motionOff ? undefined : { scale: 0.975 }}
      className="no-drag relative flex items-center justify-center rounded-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-200/80 focus-visible:ring-offset-2 focus-visible:ring-offset-base disabled:cursor-wait"
      style={{ width: size, height: size }}
    >
      {content}
    </motion.button>
  );
}

function LanternCanvas({
  active,
  busy,
  size,
  paused,
  reducedMotion,
  variant,
}: {
  active: boolean;
  busy: boolean;
  size: number;
  paused: boolean;
  reducedMotion: boolean;
  variant: "control" | "preview";
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const stateRef = useRef({ active, busy, paused, reducedMotion });
  stateRef.current = { active, busy, paused, reducedMotion };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const model = new RainCoreModel(stateRef.current.active);
    const role = variant === "control" ? "hero" : "preview";
    const baseDpr = role === "hero" ? Math.min(window.devicePixelRatio || 1, 2) : 1;
    let drops: GlassDrop[] = [];
    let spawnAccumulator = 0;
    let backingDpr = 0;
    let quality: QualityTier = "high";

    const resetDrops = () => {
      const count = variant === "preview" ? 6 : 15;
      drops = Array.from({ length: count }, () => ({
        x: 0.12 + Math.random() * 0.76,
        y: 0.08 + Math.random() * 0.72,
        radius: 0.008 + Math.random() * 0.018,
        speed: 0.012 + Math.random() * 0.028,
        alpha: 0.35 + Math.random() * 0.42,
      }));
    };
    const resizeBacking = (nextQuality: QualityTier) => {
      quality = nextQuality;
      const scale = frameQualityScale(nextQuality);
      const nextDpr = role === "hero" ? Math.max(1, baseDpr * scale) : Math.max(0.65, scale);
      if (Math.abs(nextDpr - backingDpr) < 0.001) return;
      backingDpr = nextDpr;
      canvas.width = Math.max(1, Math.round(size * nextDpr));
      canvas.height = Math.max(1, Math.round(size * nextDpr));
      ctx.setTransform(nextDpr, 0, 0, nextDpr, 0, 0);
      resetDrops();
    };
    resizeBacking("high");

    const draw = (dt: number) => {
      const state = stateRef.current;
      const staticFrame = state.paused || state.reducedMotion;
      const snapshot = model.step(dt, {
        active: state.active,
        busy: state.busy,
        reducedMotion: staticFrame,
      });
      if (!staticFrame) {
        spawnAccumulator += dt * snapshot.water * (variant === "preview" ? 0.9 : 2.2);
        if (spawnAccumulator >= 1) {
          spawnAccumulator -= 1;
          const limit = variant === "preview" ? 8 : quality === "low" ? 12 : 20;
          if (drops.length < limit) {
            drops.push({
              x: 0.12 + Math.random() * 0.76,
              y: 0.05 + Math.random() * 0.22,
              radius: 0.009 + Math.random() * 0.018,
              speed: 0.018 + Math.random() * 0.035,
              alpha: 0.4 + Math.random() * 0.35,
            });
          }
        }
        for (const drop of drops) {
          drop.y += drop.speed * (0.35 + snapshot.water * 1.5) * dt * 60;
          if (drop.y > 0.94) {
            drop.y = 0.05;
            drop.x = 0.12 + Math.random() * 0.76;
          }
        }
      }
      paintLantern(ctx, size, snapshot, drops, variant);
    };

    const loop = createRenderLoop(draw, {
      role,
      fps: variant === "preview" ? 30 : undefined,
      paused,
      onQualityChange: resizeBacking,
    });
    loopRef.current = loop;
    loop.start();
    return () => {
      loop.dispose();
      loopRef.current = null;
      drops = [];
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [size, variant]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [active, busy, paused, reducedMotion]);

  return <canvas ref={canvasRef} className="pointer-events-none absolute" style={{ width: size, height: size }} />;
}

function paintLantern(
  ctx: CanvasRenderingContext2D,
  size: number,
  snapshot: RainCoreSnapshot,
  drops: readonly GlassDrop[],
  variant: "control" | "preview",
) {
  ctx.clearRect(0, 0, size, size);
  ctx.save();
  ctx.beginPath();
  ctx.arc(size / 2, size / 2, size * 0.445, 0, Math.PI * 2);
  ctx.clip();

  const base = ctx.createRadialGradient(size * 0.54, size * 0.43, 0, size * 0.5, size * 0.5, size * 0.47);
  base.addColorStop(0, `rgba(75,92,101,${0.14 + snapshot.light * 0.08})`);
  base.addColorStop(0.48, "rgba(12,23,31,0.96)");
  base.addColorStop(1, "rgba(2,6,10,1)");
  ctx.fillStyle = base;
  ctx.fillRect(0, 0, size, size);

  const lamp = ctx.createRadialGradient(size * 0.56, size * 0.41, 0, size * 0.56, size * 0.41, size * 0.27);
  lamp.addColorStop(0, `rgba(255,190,128,${0.36 + snapshot.light * 0.58})`);
  lamp.addColorStop(0.12, `rgba(229,137,78,${snapshot.light * 0.48})`);
  lamp.addColorStop(0.42, `rgba(160,76,40,${snapshot.light * 0.16})`);
  lamp.addColorStop(1, "rgba(28,44,53,0)");
  ctx.fillStyle = lamp;
  ctx.fillRect(0, 0, size, size);

  const reflection = ctx.createLinearGradient(size * 0.56, size * 0.46, size * 0.5, size * 0.82);
  reflection.addColorStop(0, `rgba(221,137,82,${snapshot.light * 0.18})`);
  reflection.addColorStop(1, "rgba(46,64,74,0)");
  ctx.fillStyle = reflection;
  ctx.fillRect(size * 0.43, size * 0.45, size * 0.26, size * 0.4);

  for (const drop of drops) {
    const x = drop.x * size;
    const y = drop.y * size;
    const radius = drop.radius * size;
    const gradient = ctx.createRadialGradient(x - radius * 0.28, y - radius * 0.35, radius * 0.08, x, y, radius);
    gradient.addColorStop(0, `rgba(238,248,252,${drop.alpha * 0.82})`);
    gradient.addColorStop(0.32, `rgba(134,171,192,${drop.alpha * 0.13})`);
    gradient.addColorStop(0.76, `rgba(27,50,64,${drop.alpha * 0.08})`);
    gradient.addColorStop(1, `rgba(205,229,240,${drop.alpha * (0.24 + snapshot.water * 0.44)})`);
    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.ellipse(x, y, radius, radius * 1.35, 0.08, 0, Math.PI * 2);
    ctx.fill();
  }

  if (snapshot.transitionDrop != null && variant === "control") {
    const eased = snapshot.transitionDrop * snapshot.transitionDrop;
    const x = size * 0.73;
    const y = size * (0.12 + eased * 0.82);
    const radius = size * 0.046;
    ctx.strokeStyle = `rgba(177,207,222,${0.28 * (1 - snapshot.transitionDrop)})`;
    ctx.lineWidth = Math.max(1, radius * 0.28);
    ctx.beginPath();
    ctx.moveTo(x, Math.max(size * 0.1, y - size * 0.23));
    ctx.lineTo(x, y);
    ctx.stroke();
    ctx.fillStyle = "rgba(205,229,239,0.55)";
    ctx.beginPath();
    ctx.ellipse(x, y, radius, radius * 1.55, 0.04, 0, Math.PI * 2);
    ctx.fill();
  }

  const edge = ctx.createRadialGradient(size / 2, size / 2, size * 0.31, size / 2, size / 2, size * 0.46);
  edge.addColorStop(0, "rgba(0,0,0,0)");
  edge.addColorStop(0.78, "rgba(2,7,11,0.18)");
  edge.addColorStop(1, "rgba(0,2,5,0.82)");
  ctx.fillStyle = edge;
  ctx.fillRect(0, 0, size, size);
  ctx.restore();
}
