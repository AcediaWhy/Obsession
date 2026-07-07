import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { renderActive } from "../render";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// Живое ядро «Aurora»: текучие световые шторы (canvas), собранные в мягкий
// светящийся круг. Никакой геометрии дисков — только колышущийся свет, поэтому
// форма читается органично в любом состоянии. Центральный элемент айдентики.
export function AuroraCore({ active, busy = false, onClick, size = 240 }: Props) {
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
      {/* Внешнее свечение-ореол — дышит, теплеет при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(240,171,252,0.28), rgba(34,211,238,0.14) 44%, transparent 70%)"
            : "radial-gradient(circle, rgba(99,102,241,0.20), transparent 68%)",
          filter: "blur(10px)",
        }}
        animate={{
          opacity: active ? [0.7, 1, 0.7] : [0.5, 0.7, 0.5],
          scale: active ? [1, 1.05, 1] : 1,
        }}
        transition={{ duration: active ? 3.4 : 5, repeat: Infinity, ease: "easeInOut" }}
      />

      {/* Aurora-шторы внутри мягкого круга. */}
      <AuroraCanvas active={active} busy={busy} size={size} />

      {/* Тонкий ободок для «стеклянной» кромки ядра. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(240,171,252,0.28), 0 0 22px 1px rgba(34,211,238,0.35)"
            : "inset 0 0 26px 2px rgba(99,102,241,0.22), 0 0 16px 1px rgba(99,102,241,0.25)",
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
            color: active ? "#F5D0FE" : "#C7D2FE",
            textShadow: active
              ? "0 0 14px rgba(240,171,252,0.8)"
              : "0 0 12px rgba(99,102,241,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas со «шторами» полярного сияния ────────────────────────────────────

type Ribbon = {
  x: number; // центр по X, доля диаметра 0..1
  width: number; // ширина шторы, доля диаметра
  amp: number; // амплитуда бокового колыхания, доля диаметра
  freq: number; // «частота» волны по вертикали
  speed: number; // скорость дрейфа фазы
  phase: number; // стартовая фаза
  cold: [number, number, number]; // цвет в покое (r,g,b)
  hot: [number, number, number]; // цвет при активации
};

const RIBBONS: Ribbon[] = [
  { x: 0.30, width: 0.20, amp: 0.05, freq: 0.9, speed: 0.34, phase: 0.0, cold: [99, 102, 241], hot: [139, 92, 246] },
  { x: 0.46, width: 0.24, amp: 0.06, freq: 1.2, speed: 0.46, phase: 1.7, cold: [34, 211, 238], hot: [240, 171, 252] },
  { x: 0.60, width: 0.18, amp: 0.055, freq: 1.0, speed: 0.30, phase: 3.1, cold: [129, 140, 248], hot: [253, 230, 138] },
  { x: 0.70, width: 0.16, amp: 0.05, freq: 1.4, speed: 0.52, phase: 4.6, cold: [56, 189, 248], hot: [34, 211, 238] },
];

function AuroraCanvas({ active, busy, size }: { active: boolean; busy: boolean; size: number }) {
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

    let t = 0;
    let warm = 0; // 0..1 плавный переход к «разогреву»
    let raf = 0;
    let last = 0;

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0; // сброс, чтобы после паузы dt не «прыгнул»
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0.016;
      last = now;

      const { active: on, busy: loading } = stateRef.current;
      // Кадронезависимо: одинаковая скорость на 60/120/144 Гц.
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      const step = 6;
      for (const rb of RIBBONS) {
        const cx = rb.x * size;
        const half = (rb.width * size) / 2;
        const amp = rb.amp * size * (1 + warm * 0.4);
        const drift = rb.speed * (1 + warm * 0.7);

        // Центральная линия шторы: сумма двух синусов даёт «живое» колыхание.
        const centerAt = (y: number) => {
          const u = y / size;
          return (
            cx +
            Math.sin(u * Math.PI * 2 * rb.freq + t * drift + rb.phase) * amp +
            Math.sin(u * Math.PI * 3.3 * rb.freq - t * drift * 0.6 + rb.phase) * amp * 0.4
          );
        };

        // Путь-лента: вниз по левому краю, вверх по правому.
        ctx.beginPath();
        for (let y = -step; y <= size + step; y += step) {
          const x = centerAt(y) - half;
          y === -step ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        }
        for (let y = size + step; y >= -step; y -= step) {
          ctx.lineTo(centerAt(y) + half, y);
        }
        ctx.closePath();

        // Вертикальный градиент: прозрачно сверху → цвет в середине → прозрачно снизу.
        const [cr, cg, cb] = rb.cold;
        const [hr, hg, hb] = rb.hot;
        const r = Math.round(cr + (hr - cr) * warm);
        const g = Math.round(cg + (hg - cg) * warm);
        const b = Math.round(cb + (hb - cb) * warm);
        const flick = loading ? 0.75 + Math.sin(t * 6 + rb.phase) * 0.25 : 1;
        const peak = (0.16 + warm * 0.22) * flick;

        const grad = ctx.createLinearGradient(0, 0, 0, size);
        grad.addColorStop(0.0, `rgba(${r},${g},${b},0)`);
        grad.addColorStop(0.35, `rgba(${r},${g},${b},${peak})`);
        grad.addColorStop(0.6, `rgba(${r},${g},${b},${peak * 0.7})`);
        grad.addColorStop(1.0, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = grad;
        ctx.fill();
      }

      // Мягкое ядро-свечение в центре — «дышит».
      const breathe = 0.5 + Math.sin(t * 1.6) * 0.5;
      const coreR = size * (0.16 + warm * 0.05 + breathe * 0.02);
      const core = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, coreR);
      const ca = 0.35 + warm * 0.35;
      core.addColorStop(0, `rgba(${235},${240},${255},${ca})`);
      core.addColorStop(0.5, warm > 0.5 ? `rgba(240,171,252,${ca * 0.5})` : `rgba(129,140,248,${ca * 0.5})`);
      core.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = core;
      ctx.beginPath();
      ctx.arc(size / 2, size / 2, coreR, 0, Math.PI * 2);
      ctx.fill();

      // Феатеринг в мягкий круг: оставляем свет только внутри радиального маска.
      ctx.globalCompositeOperation = "destination-in";
      const mask = ctx.createRadialGradient(size / 2, size / 2, size * 0.2, size / 2, size / 2, size * 0.5);
      mask.addColorStop(0, "rgba(0,0,0,1)");
      mask.addColorStop(0.72, "rgba(0,0,0,1)");
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
