import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { renderActive } from "../render";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// Ядро темы «Russia»: одинокий тёплый огонёк в стылой ночи. Снег метёт сквозь
// круг, тёплое гало дышит. В покое свет тускл и мерцает; при активации горит
// ровно и теплее — единственный источник тепла в холоде.
export function RussiaCore({ active, busy = false, onClick, size = 240 }: Props) {
  return (
    <motion.button
      onClick={onClick}
      disabled={busy}
      whileHover={{ scale: 1.03 }}
      whileTap={{ scale: 0.97 }}
      data-snow="round"
      className="no-drag snow-surface relative flex items-center justify-center disabled:cursor-wait"
      style={{ width: size, height: size }}
    >
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(255,205,140,0.26), rgba(120,140,170,0.10) 46%, transparent 72%)"
            : "radial-gradient(circle, rgba(210,175,120,0.16), transparent 70%)",
          filter: "blur(12px)",
        }}
        animate={{ opacity: active ? [0.6, 0.9, 0.6] : [0.4, 0.6, 0.4] }}
        transition={{ duration: active ? 4.6 : 6, repeat: Infinity, ease: "easeInOut" }}
      />

      <RussiaCanvas active={active} busy={busy} size={size} />

      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(255,205,140,0.24), 0 0 20px 1px rgba(230,190,130,0.30)"
            : "inset 0 0 26px 2px rgba(150,160,185,0.16), 0 0 14px 1px rgba(150,160,185,0.18)",
          border: "1px solid rgba(255,255,255,0.05)",
        }}
        animate={{ opacity: active ? [0.75, 0.95, 0.75] : [0.5, 0.68, 0.5] }}
        transition={{ duration: 3, repeat: Infinity, ease: "easeInOut" }}
      />

      <div className="pointer-events-none absolute flex flex-col items-center">
        <span
          className="text-[11px] font-bold tracking-[0.32em]"
          style={{
            color: active ? "#F3D8AE" : "#AFB6C6",
            textShadow: active
              ? "0 0 14px rgba(255,205,140,0.75)"
              : "0 0 12px rgba(150,160,185,0.6)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas: тёплый огонёк + метель ──────────────────────────────────────────

type Flake = { x: number; y: number; r: number; vy: number; a: number };

function RussiaCanvas({ active, busy, size }: { active: boolean; busy: boolean; size: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef({ active, busy });
  stateRef.current = { active, busy };

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    const cx = size / 2;
    const cy = size / 2;
    const WIND = 0.5;

    const flakes: Flake[] = Array.from({ length: 34 }, () => ({
      x: Math.random() * size,
      y: Math.random() * size,
      r: 0.7 + Math.random() * 1.8,
      vy: size * (0.5 + Math.random() * 0.6),
      a: 0.3 + Math.random() * 0.5,
    }));

    let t = 0;
    let warm = 0;
    let raf = 0;
    let last = 0;

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0;
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.05) : 0.016;
      last = now;
      const { active: on } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      // Тёплый огонёк в центре — мерцает в покое, ровный при активации.
      const flicker = 1 - (1 - warm) * (0.18 * (0.5 + 0.5 * Math.sin(t * 8 + Math.sin(t * 2.6))));
      const breathe = 0.5 + Math.sin(t * 1.2) * 0.5;
      const coreR = size * (0.2 + warm * 0.05 + breathe * 0.02);
      const cr = Math.round(210 + 45 * warm);
      const cg = Math.round(175 + 30 * warm);
      const cb = Math.round(130 + 10 * warm);
      const ga = (0.3 + warm * 0.32) * flicker;
      const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, coreR);
      glow.addColorStop(0, `rgba(255,240,210,${ga})`);
      glow.addColorStop(0.45, `rgba(${cr},${cg},${cb},${ga * 0.6})`);
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(cx, cy, coreR, 0, Math.PI * 2);
      ctx.fill();

      // Метель поверх — холодные хлопья, теплеющие у самого огонька.
      for (const f of flakes) {
        f.y += f.vy * dt;
        f.x -= f.vy * dt * WIND;
        if (f.y > size || f.x < 0) {
          f.y = -Math.random() * size * 0.2;
          f.x = Math.random() * size;
        }
        const d = Math.hypot(f.x - cx, f.y - cy);
        const near = d < coreR ? 1 : 0;
        ctx.fillStyle = near
          ? `rgba(255,228,180,${Math.min(f.a * 1.4, 1)})`
          : `rgba(205,216,235,${f.a})`;
        ctx.beginPath();
        ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2);
        ctx.fill();
      }

      // Феатеринг в мягкий круг.
      ctx.globalCompositeOperation = "destination-in";
      const mask = ctx.createRadialGradient(cx, cy, size * 0.2, cx, cy, size * 0.5);
      mask.addColorStop(0, "rgba(0,0,0,1)");
      mask.addColorStop(0.76, "rgba(0,0,0,1)");
      mask.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = mask;
      ctx.fillRect(0, 0, size, size);
      ctx.globalCompositeOperation = "source-over";
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [size]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size }}
    />
  );
}
