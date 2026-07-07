import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { renderActive } from "../render";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// «Fallen Down» — скрытая, тихая тема. Дождь падает в тёмную гладь, рождая
// расходящиеся круги; над водой дышит мягкий отражённый свет. Приглушённая
// сумеречная палитра, медленное меланхоличное движение. При активации свет
// чуть теплеет и дождь стихает — становится спокойнее, «под защитой».
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
      {/* Мягкий ореол — дышит медленно, чуть теплеет при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(180,168,196,0.22), rgba(120,140,180,0.12) 46%, transparent 72%)"
            : "radial-gradient(circle, rgba(110,130,170,0.18), transparent 70%)",
          filter: "blur(12px)",
        }}
        animate={{ opacity: active ? [0.6, 0.85, 0.6] : [0.45, 0.62, 0.45] }}
        transition={{ duration: active ? 5 : 6.5, repeat: Infinity, ease: "easeInOut" }}
      />

      <FallenCanvas active={active} busy={busy} size={size} />

      {/* Стеклянная кромка. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(180,168,196,0.22), 0 0 20px 1px rgba(150,160,200,0.28)"
            : "inset 0 0 26px 2px rgba(110,130,170,0.18), 0 0 14px 1px rgba(110,130,170,0.20)",
          border: "1px solid rgba(255,255,255,0.05)",
        }}
        animate={{ opacity: active ? [0.7, 0.9, 0.7] : [0.5, 0.68, 0.5] }}
        transition={{ duration: 3.2, repeat: Infinity, ease: "easeInOut" }}
      />

      {/* Метка состояния. */}
      <div className="pointer-events-none absolute flex flex-col items-center" style={{ marginTop: -size * 0.14 }}>
        <span
          className="text-[11px] font-bold tracking-[0.32em]"
          style={{
            color: active ? "#D7DAEA" : "#AAB4CE",
            textShadow: active
              ? "0 0 14px rgba(180,168,196,0.7)"
              : "0 0 12px rgba(110,130,170,0.6)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas: дождь → гладь → рябь ────────────────────────────────────────────

type Drop = { x: number; y: number; len: number; speed: number };
type Ripple = { x: number; r: number; alpha: number };

function FallenCanvas({ active, busy, size }: { active: boolean; busy: boolean; size: number }) {
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
    const waterY = size * 0.6; // линия глади
    const ANGLE = 0.12; // лёгкий наклон дождя

    // Инициализация капель по всей высоте (чтобы дождь шёл сразу).
    const N = 15;
    const drops: Drop[] = Array.from({ length: N }, () => ({
      x: Math.random() * size,
      y: Math.random() * waterY,
      len: size * (0.04 + Math.random() * 0.05),
      speed: size * (0.9 + Math.random() * 0.7),
    }));
    const ripples: Ripple[] = [];

    let t = 0;
    let warm = 0; // 0..1 «спокойствие/тепло» при активации
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
      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, size, size);

      // Тёмная гладь ниже линии воды (спокойный вертикальный градиент).
      const water = ctx.createLinearGradient(0, waterY, 0, size);
      water.addColorStop(0, "rgba(24,30,46,0.9)");
      water.addColorStop(1, "rgba(10,13,22,0.95)");
      ctx.fillStyle = water;
      ctx.fillRect(0, waterY, size, size - waterY);

      // Отражённый мягкий свет над водой — «дышит», чуть теплеет.
      ctx.globalCompositeOperation = "lighter";
      const breathe = 0.5 + Math.sin(t * 0.9) * 0.5;
      const glowY = waterY - size * 0.12;
      const glowR = size * (0.24 + warm * 0.04 + breathe * 0.03);
      const cr = Math.round(120 + 60 * warm);
      const cg = Math.round(140 + 25 * warm);
      const cb = Math.round(180 + 10 * warm);
      const glow = ctx.createRadialGradient(cx, glowY, 0, cx, glowY, glowR);
      const ga = 0.22 + warm * 0.16 + breathe * 0.05;
      glow.addColorStop(0, `rgba(${cr},${cg},${cb},${ga})`);
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(cx, glowY, glowR, 0, Math.PI * 2);
      ctx.fill();

      // Отражение света в воде — вытянутая мерцающая колонна.
      const refl = ctx.createLinearGradient(0, waterY, 0, size);
      const ra = (0.1 + warm * 0.08) * (0.7 + 0.3 * Math.sin(t * 2.1));
      refl.addColorStop(0, `rgba(${cr},${cg},${cb},${ra})`);
      refl.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = refl;
      ctx.fillRect(cx - size * 0.10, waterY, size * 0.20, size - waterY);

      // Дождь: тонкие штрихи. При активации редеет и замедляется (спокойнее).
      const rainAlpha = (0.5 - warm * 0.28) * (loading ? 0.6 : 1);
      const speedK = 1 - warm * 0.35;
      ctx.strokeStyle = `rgba(170,192,220,${rainAlpha})`;
      ctx.lineWidth = Math.max(1, size * 0.006);
      ctx.beginPath();
      for (const d of drops) {
        d.y += d.speed * speedK * dt;
        d.x += d.speed * speedK * dt * ANGLE;
        if (d.y >= waterY) {
          // Капля коснулась глади — рождаем круг и перезапускаем сверху.
          ripples.push({ x: d.x, r: size * 0.008, alpha: 0.5 - warm * 0.2 });
          d.y = -d.len - Math.random() * size * 0.2;
          d.x = Math.random() * size;
          d.speed = size * (0.9 + Math.random() * 0.7);
          continue;
        }
        ctx.moveTo(d.x, d.y);
        ctx.lineTo(d.x - d.len * ANGLE, d.y - d.len);
      }
      ctx.stroke();

      // Рябь на воде: расходящиеся затухающие круги (сплюснуты перспективой).
      for (let i = ripples.length - 1; i >= 0; i--) {
        const rp = ripples[i];
        rp.r += size * 0.12 * dt;
        rp.alpha -= dt * 0.5;
        if (rp.alpha <= 0 || rp.r > size * 0.16) {
          ripples.splice(i, 1);
          continue;
        }
        ctx.strokeStyle = `rgba(180,200,225,${Math.max(rp.alpha, 0)})`;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.ellipse(rp.x, waterY, rp.r, rp.r * 0.34, 0, 0, Math.PI * 2);
        ctx.stroke();
      }

      // Линия глади — тонкий световой блик.
      const surf = ctx.createLinearGradient(0, 0, size, 0);
      surf.addColorStop(0, "rgba(150,170,205,0)");
      surf.addColorStop(0.5, `rgba(170,190,220,${0.25 + warm * 0.1})`);
      surf.addColorStop(1, "rgba(150,170,205,0)");
      ctx.strokeStyle = surf;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(0, waterY);
      ctx.lineTo(size, waterY);
      ctx.stroke();

      // Феатеринг в мягкий круг.
      ctx.globalCompositeOperation = "destination-in";
      const mask = ctx.createRadialGradient(cx, size / 2, size * 0.2, cx, size / 2, size * 0.5);
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
