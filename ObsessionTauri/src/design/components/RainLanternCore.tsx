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
import { createSpriteCache } from "./glowSprite";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  variant?: "control" | "preview";
};

// Ядро Rain рисует круги на воде в холодных цветах темы japan.
// Яркость зависит от snapshot.light; при включении transitionDrop создаёт
// крупное кольцо. Элементы кадра берутся из кэша спрайтов.

// Холодная палитра темы Rain (globals.css [data-theme="japan"]).
const CREST = "210, 228, 244"; // серебристо-синий гребень волны
const DEEP = "150, 182, 212"; // accent-cyan — свечение глубины

const clamp01 = (x: number) => Math.max(0, Math.min(1, x));
const smoothstep = (a: number, b: number, x: number) => {
  const t = clamp01((x - a) / (b - a));
  return t * t * (3 - 2 * t);
};

// ─── Модульные кэш-спрайты (шарятся между всеми инстансами, вкл. 6 превью) ─────

// Тёмная сине-серая гладь воды: центр чуть светлее (глубина светится), к краю
// уходит в почти чёрный сланец. Мягко, без жёстких границ-«объекта».
const water = createSpriteCache(1, 256, (sctx, px) => {
  const R = px / 2;
  const g = sctx.createRadialGradient(R, R * 0.94, 0, R, R, R);
  g.addColorStop(0, "rgba(24, 34, 48, 0.92)");
  g.addColorStop(0.55, "rgba(11, 18, 28, 0.96)");
  g.addColorStop(1, "rgba(4, 8, 14, 1)");
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

// Волна-кольцо: мягкий тонкий светящийся гребень (прозрачно внутри → серебристо-
// синий пик у ~0.8R → прозрачно снаружи). Масштабируется по радиусу ряби —
// растёт и физично размывается. Цвет холодный, запечён; яркость даём globalAlpha.
const wave = createSpriteCache(1, 256, (sctx, px) => {
  const R = px / 2;
  const g = sctx.createRadialGradient(R, R, 0, R, R, R);
  g.addColorStop(0.0, `rgba(${CREST}, 0)`);
  g.addColorStop(0.62, `rgba(${CREST}, 0)`);
  g.addColorStop(0.8, `rgba(${CREST}, 0.9)`);
  g.addColorStop(0.9, `rgba(${DEEP}, 0.32)`);
  g.addColorStop(1.0, `rgba(${DEEP}, 0)`);
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

// Мягкое пятно-свечение: дышащее ядро глубины + короткая вспышка на удар капли.
const spark = createSpriteCache(1, 128, (sctx, px) => {
  const R = px / 2;
  const g = sctx.createRadialGradient(R, R, 0, R, R, R);
  g.addColorStop(0, `rgba(${CREST}, 0.85)`);
  g.addColorStop(0.4, `rgba(${DEEP}, 0.3)`);
  g.addColorStop(1, `rgba(${DEEP}, 0)`);
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

// Источники капель — детерминированная раскладка (доля размера от центра). Разные
// НЕсоизмеримые периоды → кольца никогда не синхронны, вода «живая». Ноль RNG.
const SOURCES = [
  { dx: 0.0, dy: -0.02, period: 2.6, phase: 0.0, maxR: 0.46, strength: 1.0 },
  { dx: -0.17, dy: 0.15, period: 3.3, phase: 1.3, maxR: 0.26, strength: 0.6 },
  { dx: 0.18, dy: -0.14, period: 3.9, phase: 2.7, maxR: 0.24, strength: 0.55 },
] as const;

const RIPPLE_LIFE = 3.2; // сколько секунд живёт одно кольцо

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
            ? "radial-gradient(circle, rgba(150,182,212,0.18), rgba(122,152,190,0.08) 48%, transparent 72%)"
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
            ? "inset 0 0 30px rgba(150,182,212,0.10), 0 0 18px rgba(122,152,190,0.14)"
            : "inset 0 0 30px rgba(148,176,193,0.08), 0 0 12px rgba(91,118,137,0.10)",
        }}
      />
      <span
        className="pointer-events-none absolute rounded-full px-2 py-1 text-2xs font-bold tracking-[0.32em]"
        style={{
          bottom: size * 0.21,
          color: active ? "#DCEAF6" : "#C1D0DA",
          background: "rgba(3,7,11,0.24)",
          textShadow: active ? "0 0 12px rgba(150,182,212,0.55)" : "0 0 10px rgba(130,158,177,0.48)",
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
    let backingDpr = 0;
    let mask: CanvasGradient | null = null;

    const resizeBacking = (nextQuality: QualityTier) => {
      const scale = frameQualityScale(nextQuality);
      const nextDpr = role === "hero" ? Math.max(1, baseDpr * scale) : Math.max(0.65, scale);
      if (Math.abs(nextDpr - backingDpr) < 0.001) return;
      backingDpr = nextDpr;
      canvas.width = Math.max(1, Math.round(size * nextDpr));
      canvas.height = Math.max(1, Math.round(size * nextDpr));
      ctx.setTransform(nextDpr, 0, 0, nextDpr, 0, 0);
      mask = ctx.createRadialGradient(size / 2, size / 2, size * 0.2, size / 2, size / 2, size * 0.47);
      mask.addColorStop(0, "rgba(0,0,0,1)");
      mask.addColorStop(0.74, "rgba(0,0,0,1)");
      mask.addColorStop(1, "rgba(0,0,0,0)");
    };
    resizeBacking("high");

    let t = 0;
    const draw = (dt: number) => {
      const state = stateRef.current;
      const staticFrame = state.paused || state.reducedMotion;
      const snapshot = model.step(dt, {
        active: state.active,
        busy: state.busy,
        reducedMotion: staticFrame,
      });
      if (!staticFrame) t += dt;
      paintRipples(ctx, size, snapshot, t, mask, staticFrame, variant);
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

function paintRipples(
  ctx: CanvasRenderingContext2D,
  size: number,
  snapshot: RainCoreSnapshot,
  t: number,
  mask: CanvasGradient | null,
  poster: boolean,
  variant: "control" | "preview",
) {
  // Яркость колец возрастает вместе со значением snapshot.light.
  const energy = 0.14 + clamp01((snapshot.light - 0.12) / 0.88) * 0.86;
  const cx = size / 2;
  const cy = size / 2;

  ctx.clearRect(0, 0, size, size);
  ctx.globalCompositeOperation = "source-over";
  ctx.globalAlpha = 1;
  ctx.drawImage(water(0), 0, 0, size, size);

  ctx.globalCompositeOperation = "lighter";

  // Дышащее ядро глубины — центр воды мягко пульсирует светом.
  const breathe = poster ? 0.7 : 0.65 + 0.35 * Math.sin(t * 1.2);
  const cr = size * 0.16;
  ctx.globalAlpha = (0.1 + energy * 0.34) * breathe;
  ctx.drawImage(spark(0), cx - cr, cy - cr, cr * 2, cr * 2);

  if (poster) {
    // Замерзшая рябь: детерминированный набор колец из центра.
    for (let i = 0; i < 3; i += 1) {
      const r = size * (0.16 + i * 0.11);
      ctx.globalAlpha = energy * (0.5 - i * 0.13);
      ctx.drawImage(wave(0), cx - r, cy - r, r * 2, r * 2);
    }
  } else {
    for (const s of SOURCES) {
      drawSource(ctx, size, cx + s.dx * size, cy + s.dy * size, s, t, energy);
    }
    if (variant === "control" && snapshot.transitionDrop != null) {
      drawActivation(ctx, size, cx, cy, snapshot.transitionDrop, energy);
    }
  }

  if (mask) {
    ctx.globalCompositeOperation = "destination-in";
    ctx.globalAlpha = 1;
    ctx.fillStyle = mask;
    ctx.fillRect(0, 0, size, size);
  }
  ctx.globalCompositeOperation = "source-over";
  ctx.globalAlpha = 1;
}

function drawSource(
  ctx: CanvasRenderingContext2D,
  size: number,
  cx: number,
  cy: number,
  src: (typeof SOURCES)[number],
  t: number,
  energy: number,
) {
  const gen = Math.floor((t + src.phase) / src.period);
  const back = Math.ceil(RIPPLE_LIFE / src.period);
  for (let k = gen; k >= gen - back; k -= 1) {
    const age = t + src.phase - k * src.period;
    if (age < 0 || age > RIPPLE_LIFE) continue;
    const p = age / RIPPLE_LIFE;
    const r = src.maxR * size * (1 - (1 - p) * (1 - p)); // ease-out: волна замедляется
    const a = smoothstep(0, 0.07, p) * Math.pow(1 - p, 1.5) * energy * src.strength;
    if (a <= 0.003) continue;
    ctx.globalAlpha = a;
    ctx.drawImage(wave(0), cx - r, cy - r, r * 2, r * 2);
    // Удар капли: короткая вспышка в точке рождения кольца.
    if (age < 0.22) {
      const fr = size * 0.05;
      ctx.globalAlpha = (1 - age / 0.22) * energy * src.strength * 0.7;
      ctx.drawImage(spark(0), cx - fr, cy - fr, fr * 2, fr * 2);
    }
  }
}

function drawActivation(
  ctx: CanvasRenderingContext2D,
  size: number,
  cx: number,
  cy: number,
  drop: NonNullable<RainCoreSnapshot["transitionDrop"]>,
  energy: number,
) {
  // Включение обхода: капля упала в центр — крупное яркое кольцо расходится
  // (fall 0→1), затем тает (alpha). Один акцент, ровно концепция «круги по воде».
  const p = drop.fall;
  const r = size * 0.46 * (1 - (1 - p) * (1 - p));
  ctx.globalAlpha = drop.alpha * (0.5 + energy * 0.5);
  ctx.drawImage(wave(0), cx - r, cy - r, r * 2, r * 2);
  if (p < 0.32) {
    const fr = size * 0.09;
    ctx.globalAlpha = (1 - p / 0.32) * drop.alpha;
    ctx.drawImage(spark(0), cx - fr, cy - fr, fr * 2, fr * 2);
  }
}
