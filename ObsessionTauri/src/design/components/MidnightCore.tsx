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
import { createSpriteCache } from "./glowSprite";
import { CoreShell } from "./CoreShell";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  interactive?: boolean;
};

// Живое ядро «Midnight»: фонарь в ночном тумане. Холодная лампа в верхней трети,
// вниз — конус света, в котором дрейфует морось. Активация — лампа разгорается,
// конус плотнее; busy — лампа нервно фликерит, как перед перегоранием.
export function MidnightCore({
  active,
  busy = false,
  onClick,
  size = 240,
  paused = false,
  interactive = true,
}: Props) {
  // Ореолы гасим, когда окно скрыто ИЛИ это застывшее превью (paused):
  // framer-motion гоняет их на компоновщике (WAAPI) и сам на скрытие не реагирует.
  const renderOn = useRenderActive() && !paused;
  return (
    <CoreShell
      interactive={interactive}
      onClick={onClick}
      busy={busy}
      size={size}
    >
      {/* Внешнее свечение-ореол — холодный свет, рассеянный туманом. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(200,218,238,0.26), rgba(150,172,196,0.12) 46%, transparent 72%)"
            : "radial-gradient(circle, rgba(168,186,206,0.15), transparent 68%)",
          filter: "blur(10px)",
        }}
        animate={
          renderOn
            ? {
                opacity: active ? [0.7, 1, 0.7] : [0.45, 0.65, 0.45],
                scale: active ? [1, 1.04, 1] : 1,
              }
            : { opacity: active ? 0.85 : 0.55, scale: 1 }
        }
        transition={
          renderOn
            ? { duration: active ? 3.8 : 5.5, repeat: Infinity, ease: "easeInOut" }
            : { duration: 0.3 }
        }
      />

      {/* Лампа, конус света и морось. */}
      <MidnightCanvas active={active} busy={busy} size={size} paused={paused} />

      {/* Тонкий ободок «стеклянной» кромки ядра. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(200,218,238,0.22), 0 0 22px 1px rgba(168,190,214,0.28)"
            : "inset 0 0 26px 2px rgba(168,186,206,0.16), 0 0 16px 1px rgba(130,148,168,0.20)",
          border: "1px solid rgba(255,255,255,0.06)",
        }}
        animate={renderOn ? { opacity: active ? [0.8, 1, 0.8] : [0.5, 0.7, 0.5] } : { opacity: active ? 0.9 : 0.6 }}
        transition={renderOn ? { duration: 3, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      {/* Метка состояния. */}
      <div className="pointer-events-none absolute flex flex-col items-center">
        <span
          className="text-2xs font-bold tracking-[0.32em]"
          style={{
            color: active ? "#EAF2FA" : "#C9D6E2",
            textShadow: active
              ? "0 0 14px rgba(200,218,238,0.85)"
              : "0 0 12px rgba(160,180,200,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </CoreShell>
  );
}

// ─── Canvas с лампой, конусом света и моросью ────────────────────────────────

type Drop = { x: number; y: number; vy: number; drift: number; size: number; a: number };

// Капля мороси/крупинка тумана: холодный белый глоу-спрайт по корзинам warm —
// при активации чуть ярче к белому (см. glowSprite.ts).
const dropSprite = createSpriteCache(8, 32, (sctx, px, k) => {
  const r = px / 2;
  const c = Math.round(214 + k * 30);
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, `rgba(${c},${Math.min(255, c + 10)},255,1)`);
  g.addColorStop(1, "rgba(200,218,240,0)");
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

function MidnightCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef({ active, busy });
  stateRef.current = { active, busy };
  const loopRef = useRef<RenderLoop | null>(null);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Превью стартует в 1× и может снизиться до 0.65×; hero использует DPR
    // до 2×, но никогда не опускается ниже 1×.
    const role = size >= CORE_HERO_MIN_SIZE ? "hero" : "preview";
    const baseDpr = role === "hero" ? Math.min(window.devicePixelRatio || 1, 2) : 1;
    let backingDpr = 0;
    let mask: CanvasGradient;
    const resizeBacking = (qualityTier: QualityTier) => {
      const qualityScale = frameQualityScale(qualityTier);
      const nextDpr = role === "hero"
        ? Math.max(1, baseDpr * qualityScale)
        : Math.max(0.65, qualityScale);
      if (Math.abs(nextDpr - backingDpr) < 0.001) return;
      backingDpr = nextDpr;
      canvas.width = Math.max(1, Math.round(size * backingDpr));
      canvas.height = Math.max(1, Math.round(size * backingDpr));
      ctx.setTransform(backingDpr, 0, 0, backingDpr, 0, 0);
      mask = ctx.createRadialGradient(size / 2, size / 2, size * 0.2, size / 2, size / 2, size * 0.5);
      mask.addColorStop(0, "rgba(0,0,0,1)");
      mask.addColorStop(0.72, "rgba(0,0,0,1)");
      mask.addColorStop(1, "rgba(0,0,0,0)");
    };
    resizeBacking("high");

    const cx = size / 2;
    const cy = size / 2;
    // Лампа в верхней трети, конус раскрывается к «земле» у нижней кромки диска.
    const lampY = cy - size * 0.2;
    const floorY = cy + size * 0.34;
    const coneHalf = size * 0.26; // полуширина конуса у земли

    // Морось: падает сквозь конус, вне конуса гаснет.
    const spawn = (d: Drop) => {
      d.x = cx + (Math.random() - 0.5) * size * 0.56;
      d.y = lampY - size * 0.04 + Math.random() * size * 0.1;
      d.vy = size * (0.16 + Math.random() * 0.2);
      d.drift = (Math.random() - 0.5) * size * 0.02;
      d.size = 0.6 + Math.random() * 1.1;
      d.a = 0.4 + Math.random() * 0.6;
    };
    const drops: Drop[] = Array.from({ length: 18 }, () => {
      const d: Drop = { x: 0, y: 0, vy: 0, drift: 0, size: 0, a: 0 };
      spawn(d);
      d.y = lampY + Math.random() * (floorY - lampY);
      return d;
    });

    let t = 0;
    // Сеем от текущего состояния — маунт при включённом щите сразу тёплый.
    let warm = stateRef.current.active ? 1 : 0;


    const draw = (dt: number) => {
      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      // Ровное «гудение» лампы: лёгкая медленная зыбь; busy — рваный фликер.
      let hum = 0.92 + 0.05 * Math.sin(t * 1.3) + 0.03 * Math.sin(t * 4.7 + 1.4);
      if (loading) hum *= 0.55 + 0.45 * Math.abs(Math.sin(t * 21));

      // Ореол лампы в тумане.
      const glowR = size * (0.15 + warm * 0.05) * hum;
      const glowA = (0.55 + warm * 0.35) * hum;
      const glow = ctx.createRadialGradient(cx, lampY, 0, cx, lampY, glowR);
      glow.addColorStop(0, `rgba(236,244,252,${glowA})`);
      glow.addColorStop(0.35, `rgba(200,218,238,${glowA * 0.6})`);
      glow.addColorStop(1, "rgba(160,182,204,0)");
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(cx, lampY, glowR, 0, Math.PI * 2);
      ctx.fill();

      // Сама лампа — яркая короткая перекладина, как двойной светильник в кадре.
      ctx.fillStyle = `rgba(244,250,255,${Math.min(1, glowA * 1.4)})`;
      const lw = size * 0.1;
      ctx.fillRect(cx - lw / 2, lampY - size * 0.008, lw, size * 0.016);

      // Конус света: от лампы к земле, тает книзу. Туман «дышит» шириной.
      const sway = 1 + 0.04 * Math.sin(t * 0.6);
      const cone = ctx.createLinearGradient(0, lampY, 0, floorY);
      cone.addColorStop(0, `rgba(214,228,244,${(0.30 + warm * 0.16) * hum})`);
      cone.addColorStop(0.6, `rgba(190,208,228,${(0.10 + warm * 0.07) * hum})`);
      cone.addColorStop(1, "rgba(170,190,212,0)");
      ctx.fillStyle = cone;
      ctx.beginPath();
      ctx.moveTo(cx - lw * 0.5, lampY);
      ctx.lineTo(cx + lw * 0.5, lampY);
      ctx.lineTo(cx + coneHalf * sway, floorY);
      ctx.lineTo(cx - coneHalf * sway, floorY);
      ctx.closePath();
      ctx.fill();

      // Морось: видима в конусе, за его кромкой гаснет.
      for (const d of drops) {
        d.y += d.vy * dt;
        d.x += d.drift * dt;
        if (d.y > floorY) spawn(d);
        const prog = Math.max(0, Math.min(1, (d.y - lampY) / (floorY - lampY)));
        const half = lw * 0.5 + (coneHalf * sway - lw * 0.5) * prog;
        const inside = Math.max(0, 1 - Math.abs(d.x - cx) / (half + size * 0.02));
        const da = Math.min(1, d.a * inside * hum * (0.5 + warm * 0.45));
        if (da <= 0.01) continue;
        const r = d.size;
        const R = r * 3;
        ctx.globalAlpha = da;
        ctx.drawImage(dropSprite(warm), d.x - R, d.y - R, R * 2, R * 2);
      }
      ctx.globalAlpha = 1;

      // Феатеринг в мягкий круг.
      ctx.globalCompositeOperation = "destination-in";
      ctx.fillStyle = mask;
      ctx.fillRect(0, 0, size, size);
      ctx.globalCompositeOperation = "source-over";
    };
    // paused НЕ в deps: ховер не пересоздаёт эффект. В трее цикл гасит общий scheduler.
    const loop = createRenderLoop(draw, {
      role,
      onQualityChange: resizeBacking,
      paused: pausedRef.current,
    });
    loopRef.current = loop;
    loop.start();
    return () => {
      loop.dispose();
      loopRef.current = null;
    };
  }, [size]);

  // Пауза/продолжение без тир-дауна эффекта: состояние анимации (t/warm) живёт.
  useEffect(() => {
    loopRef.current?.setPaused(paused);
  }, [paused]);

  // Застывшее превью перерисовываем при смене active/busy.
  useEffect(() => {
    // Живой цикл: no-op; замерший (paused/reduce-motion) — дорисовать кадр.
    loopRef.current?.invalidate();
  }, [active, busy, paused]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size }}
    />
  );
}
