import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { renderActive } from "../render";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// «Офаним» — престольное колесо (Иез. 1): колёса-в-колёсах, вращающиеся сквозь
// друг друга под разными осями, усеянные глазами, в ореоле глориоли. В покое —
// тускло-золотое, глаза прикрыты; при активации разгоняется, теплеет в магенту,
// глаза раскрываются. Альтернативный hero к «Aurora», та же семантика состояний.
export function OphanimCore({ active, busy = false, onClick, size = 240 }: Props) {
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
      {/* Глориоль — внешний ореол святости, дышит и теплеет при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(240,171,252,0.28), rgba(253,224,71,0.14) 44%, transparent 70%)"
            : "radial-gradient(circle, rgba(253,224,71,0.18), transparent 68%)",
          filter: "blur(10px)",
        }}
        animate={{
          opacity: active ? [0.7, 1, 0.7] : [0.5, 0.7, 0.5],
          scale: active ? [1, 1.05, 1] : 1,
        }}
        transition={{ duration: active ? 3.4 : 5, repeat: Infinity, ease: "easeInOut" }}
      />

      {/* Колёса-в-колёсах на canvas. */}
      <OphanimCanvas active={active} busy={busy} size={size} />

      {/* Стеклянная кромка престола. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(240,171,252,0.26), 0 0 22px 1px rgba(253,224,71,0.35)"
            : "inset 0 0 26px 2px rgba(253,224,71,0.20), 0 0 16px 1px rgba(234,179,8,0.22)",
          border: "1px solid rgba(255,255,255,0.06)",
        }}
        animate={{ opacity: active ? [0.8, 1, 0.8] : [0.55, 0.75, 0.55] }}
        transition={{ duration: 2.6, repeat: Infinity, ease: "easeInOut" }}
      />

      {/* Метка состояния. */}
      <div className="pointer-events-none absolute flex flex-col items-center">
        <span
          className="text-[11px] font-bold tracking-[0.32em]"
          style={{
            color: active ? "#F5D0FE" : "#FDE68A",
            textShadow: active
              ? "0 0 14px rgba(240,171,252,0.8)"
              : "0 0 12px rgba(234,179,8,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas: колёса-в-колёсах с глазами ──────────────────────────────────────

type Wheel = {
  r: number; // радиус, доля диаметра 0..0.5
  axis: number; // ориентация большой оси эллипса (рад)
  tiltSpeed: number; // скорость «переворота» в 3D
  tiltPhase: number; // фаза переворота (чтобы не проходили ребром разом)
  spin: number; // скорость бега глаз по ободу
  eyes: number; // число глаз на ободе
  lw: number; // толщина обода, доля диаметра
  cold: [number, number, number]; // цвет в покое
  hot: [number, number, number]; // цвет при активации
};

const WHEELS: Wheel[] = [
  { r: 0.42, axis: 0.0, tiltSpeed: 0.5, tiltPhase: 0.0, spin: 0.25, eyes: 12, lw: 0.012, cold: [234, 179, 8], hot: [240, 171, 252] },
  { r: 0.42, axis: Math.PI / 2, tiltSpeed: 0.44, tiltPhase: 1.9, spin: -0.3, eyes: 12, lw: 0.012, cold: [253, 224, 71], hot: [232, 121, 249] },
  { r: 0.30, axis: Math.PI / 4, tiltSpeed: 0.7, tiltPhase: 3.3, spin: 0.42, eyes: 8, lw: 0.014, cold: [250, 204, 21], hot: [217, 70, 239] },
  { r: 0.30, axis: -Math.PI / 4, tiltSpeed: 0.62, tiltPhase: 4.7, spin: -0.5, eyes: 8, lw: 0.014, cold: [253, 230, 138], hot: [244, 114, 182] },
];

function OphanimCanvas({ active, busy, size }: { active: boolean; busy: boolean; size: number }) {
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
    let t = 0;
    let warm = 0; // 0..1 плавный «разогрев»
    let raf = 0;
    let last = 0;

    // Точка на наклонённом эллипсе: локальные (rx·cosA, ry·sinA) → поворот на axis.
    const ellipsePt = (a: number, rx: number, ry: number, axis: number) => {
      const lx = rx * Math.cos(a);
      const ly = ry * Math.sin(a);
      const c = Math.cos(axis);
      const s = Math.sin(axis);
      return { x: cx + lx * c - ly * s, y: cy + lx * s + ly * c };
    };

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0; // сброс, чтобы после паузы dt не «прыгнул»
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0.016;
      last = now;

      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      const spinBoost = 1 + warm * 1.1; // при активации всё крутится быстрее

      for (const wh of WHEELS) {
        const rx = wh.r * size;
        // ry «переворачивается» в 3D: от ребра (~0.12) до полного круга.
        const yScale = 0.12 + 0.88 * Math.abs(Math.sin(t * wh.tiltSpeed * spinBoost + wh.tiltPhase));
        const ry = rx * yScale;

        // Цвет обода: лерп cold→hot по warm.
        const [cr, cg, cb] = wh.cold;
        const [hr, hg, hb] = wh.hot;
        const r = Math.round(cr + (hr - cr) * warm);
        const g = Math.round(cg + (hg - cg) * warm);
        const b = Math.round(cb + (hb - cb) * warm);

        // Обод колеса: два штриха — широкий тусклый (свечение) + узкий яркий.
        const rimA = 0.10 + warm * 0.16;
        ctx.beginPath();
        for (let i = 0; i <= 64; i++) {
          const a = (i / 64) * Math.PI * 2;
          const p = ellipsePt(a, rx, ry, wh.axis);
          i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y);
        }
        ctx.closePath();
        ctx.strokeStyle = `rgba(${r},${g},${b},${rimA * 0.5})`;
        ctx.lineWidth = wh.lw * size * 3;
        ctx.stroke();
        ctx.strokeStyle = `rgba(${Math.min(r + 30, 255)},${Math.min(g + 30, 255)},${b},${rimA})`;
        ctx.lineWidth = wh.lw * size;
        ctx.stroke();

        // Глаза по ободу — бегут по кругу, раскрываются с warm, «моргают».
        const openBase = 0.18 + warm * 0.82;
        for (let e = 0; e < wh.eyes; e++) {
          const a = (e / wh.eyes) * Math.PI * 2 + t * wh.spin * spinBoost;
          const p = ellipsePt(a, rx, ry, wh.axis);
          // Глаза на «дальней» стороне (верх наклона) тусклее → ощущение объёма.
          const depth = 0.55 + 0.45 * (0.5 + 0.5 * Math.sin(a + wh.axis));
          // Индивидуальное моргание + бегущая вспышка при busy.
          const blink = 0.5 + 0.5 * Math.sin(t * 2.2 + e * 1.3 + wh.tiltPhase);
          const chase = loading
            ? Math.max(0, Math.sin(t * 5 - e * (6.283 / wh.eyes)))
            : 0;
          const open = Math.min(1, openBase * (0.6 + 0.4 * blink) + chase * 0.6);
          const rad = wh.lw * size * (1.6 + open * 1.6);

          const glow = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, rad * 2.4);
          const ea = (0.22 + warm * 0.5) * depth * (0.4 + 0.6 * open);
          glow.addColorStop(0, `rgba(255,255,255,${Math.min(ea * 1.4, 1)})`);
          glow.addColorStop(0.4, `rgba(${r},${g},${b},${ea})`);
          glow.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = glow;
          ctx.beginPath();
          ctx.arc(p.x, p.y, rad * 2.4, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      // Глориоль-ядро в центре — «дышит».
      const breathe = 0.5 + Math.sin(t * 1.6) * 0.5;
      const coreR = size * (0.15 + warm * 0.05 + breathe * 0.02);
      const core = ctx.createRadialGradient(cx, cy, 0, cx, cy, coreR);
      const ca = 0.34 + warm * 0.36;
      core.addColorStop(0, `rgba(255,251,235,${ca})`);
      core.addColorStop(0.5, warm > 0.5 ? `rgba(240,171,252,${ca * 0.5})` : `rgba(250,204,21,${ca * 0.5})`);
      core.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = core;
      ctx.beginPath();
      ctx.arc(cx, cy, coreR, 0, Math.PI * 2);
      ctx.fill();

      // Феатеринг в мягкий круг: гасим всё за пределами радиального маска.
      ctx.globalCompositeOperation = "destination-in";
      const mask = ctx.createRadialGradient(cx, cy, size * 0.2, cx, cy, size * 0.5);
      mask.addColorStop(0, "rgba(0,0,0,1)");
      mask.addColorStop(0.78, "rgba(0,0,0,1)");
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
