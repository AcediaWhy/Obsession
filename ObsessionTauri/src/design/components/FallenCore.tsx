import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { renderActive } from "../render";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// Пиксельное сердце SOUL — визитная карточка Undertale, нарисованное блоками по
// сетке (без сглаживания). Единственный цвет в монохромной теме: приглушённо-алое
// в покое (медленное сердцебиение, меланхолия), яркое и тёплое при активации
// (DETERMINATION). Форма намеренно грубо-пиксельная.
const HEART: number[][] = [
  [0, 1, 1, 0, 0, 1, 1, 0],
  [1, 1, 1, 1, 1, 1, 1, 1],
  [1, 1, 1, 1, 1, 1, 1, 1],
  [1, 1, 1, 1, 1, 1, 1, 1],
  [0, 1, 1, 1, 1, 1, 1, 0],
  [0, 0, 1, 1, 1, 1, 0, 0],
  [0, 0, 0, 1, 1, 0, 0, 0],
];
const COLS = HEART[0].length;
const ROWS = HEART.length;

export function FallenCore({ active, busy = false, onClick, size = 240 }: Props) {
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
      {/* Мягкий алый ореол — дышит, теплеет и ярче при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(255,58,64,0.30), rgba(150,30,40,0.12) 48%, transparent 72%)"
            : "radial-gradient(circle, rgba(150,36,44,0.18), transparent 70%)",
          filter: "blur(12px)",
        }}
        animate={{ opacity: active ? [0.6, 0.9, 0.6] : [0.4, 0.58, 0.4] }}
        transition={{ duration: active ? 4.2 : 6, repeat: Infinity, ease: "easeInOut" }}
      />

      <SoulCanvas active={active} busy={busy} size={size} />

      {/* Стеклянная кромка. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(255,58,64,0.20), 0 0 20px 1px rgba(220,54,62,0.26)"
            : "inset 0 0 26px 2px rgba(150,36,44,0.16), 0 0 14px 1px rgba(150,36,44,0.18)",
          border: "1px solid rgba(255,255,255,0.05)",
        }}
        animate={{ opacity: active ? [0.7, 0.9, 0.7] : [0.5, 0.66, 0.5] }}
        transition={{ duration: 3.2, repeat: Infinity, ease: "easeInOut" }}
      />

      {/* Метка состояния под сердцем. */}
      <div className="pointer-events-none absolute flex flex-col items-center" style={{ marginTop: size * 0.34 }}>
        <span
          className="text-[11px] font-bold tracking-[0.32em]"
          style={{
            color: active ? "#FFC4C7" : "#B79398",
            textShadow: active
              ? "0 0 14px rgba(255,58,64,0.7)"
              : "0 0 12px rgba(150,36,44,0.55)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas: пиксельное сердце с сердцебиением ───────────────────────────────

function SoulCanvas({ active, busy, size }: { active: boolean; busy: boolean; size: number }) {
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
    ctx.imageSmoothingEnabled = false; // крипкие пиксели

    const cx = size / 2;
    const cy = size / 2;

    let t = 0;
    let warm = 0; // 0..1 плавный «разогрев» при активации
    let raf = 0;
    let last = 0;

    // Сердцебиение: два толчка (lub-dub) и пауза, свёрнутые в фазу 0..1.
    const heartbeat = (u: number) => {
      const g = (c: number, s: number) => Math.exp(-((u - c) * (u - c)) / (s * s));
      return g(0.0, 0.055) + g(0.17, 0.06) * 0.62;
    };

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0;
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.05) : 0.016;
      last = now;
      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      const period = loading ? 0.7 : 1.9 - warm * 0.7;
      const u = (t % period) / period;
      const beat = heartbeat(u); // 0..~1.6

      ctx.clearRect(0, 0, size, size);

      // Пульс масштаба (пиксели остаются квадратными → чёткими).
      const scale = 1 + beat * (0.05 + warm * 0.05);
      const heartW = size * 0.42 * scale;
      const px = heartW / COLS;
      const heartH = px * ROWS;
      const x0 = cx - heartW / 2;
      const y0 = cy - heartH / 2;

      // Цвет: приглушённо-алый в покое → яркий алый при DETERMINATION.
      const cr = Math.round(178 + (255 - 178) * warm);
      const cg = Math.round(40 + (58 - 40) * warm);
      const cb = Math.round(48 + (60 - 48) * warm);

      // Внешнее свечение (additive), пульсирует с биением.
      ctx.globalCompositeOperation = "lighter";
      const glowR = size * (0.2 + warm * 0.05) * (1 + beat * 0.16);
      const ga = 0.14 + warm * 0.2 + beat * 0.14;
      const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, glowR);
      glow.addColorStop(0, `rgba(${cr},${cg},${cb},${ga})`);
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(cx - glowR, cy - glowR, glowR * 2, glowR * 2);

      // Тело сердца — плоские красные блоки.
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = `rgb(${cr},${cg},${cb})`;
      for (let r = 0; r < ROWS; r++) {
        for (let c = 0; c < COLS; c++) {
          if (!HEART[r][c]) continue;
          const bx = Math.floor(x0 + c * px);
          const by = Math.floor(y0 + r * px);
          const bw = Math.ceil(px) + 1; // +1 закрывает щели округления
          ctx.fillRect(bx, by, bw, bw);
        }
      }

      // Лёгкий верхний блик — один осветлённый ряд блоков (объём, не ломает пиксель).
      ctx.globalCompositeOperation = "lighter";
      ctx.fillStyle = `rgba(255,150,150,${0.16 + warm * 0.12})`;
      for (let c = 0; c < COLS; c++) {
        if (!HEART[1][c]) continue;
        const bx = Math.floor(x0 + c * px);
        const by = Math.floor(y0 + 1 * px);
        const bw = Math.ceil(px) + 1;
        ctx.fillRect(bx, by, bw, Math.ceil(px));
      }
      ctx.globalCompositeOperation = "source-over";
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [size]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size, imageRendering: "pixelated" }}
    />
  );
}
