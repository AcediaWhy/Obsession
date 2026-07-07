import { useEffect, useRef } from "react";
import { motion } from "framer-motion";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// Живое ядро-сингулярность: аккреционный диск + горизонт событий + фотонное
// кольцо + засасывающиеся частицы (canvas). Центральный элемент айдентики.
export function Singularity({ active, busy = false, onClick, size = 260 }: Props) {
  return (
    <motion.button
      onClick={onClick}
      disabled={busy}
      whileHover={{ scale: 1.03 }}
      whileTap={{ scale: 0.97 }}
      className="no-drag relative flex items-center justify-center disabled:cursor-wait"
      style={{ width: size, height: size }}
    >
      {/* Внешнее свечение — дышит, разогревается при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.14,
          background: active
            ? "radial-gradient(circle, rgba(240,171,252,0.30), rgba(34,211,238,0.14) 42%, transparent 68%)"
            : "radial-gradient(circle, rgba(99,102,241,0.22), transparent 66%)",
          filter: "blur(6px)",
        }}
        animate={{ opacity: active ? [0.7, 1, 0.7] : [0.5, 0.7, 0.5], scale: active ? [1, 1.06, 1] : 1 }}
        transition={{ duration: active ? 3 : 5, repeat: Infinity, ease: "easeInOut" }}
      />

      {/* Частицы, падающие в ядро. */}
      <InfallCanvas active={active} size={size} />

      {/* Аккреционный диск — наклонённый эллипс, вращается. */}
      <AccretionDisk active={active} size={size} />

      {/* Кольцо Эйнштейна — гравитационное линзирование с хром. аберрацией. */}
      <LensingRing active={active} size={size} />

      {/* Ядро: горизонт событий + фотонное кольцо. */}
      <div
        className="relative flex items-center justify-center rounded-full"
        style={{ width: size * 0.42, height: size * 0.42 }}
      >
        {/* Фотонное кольцо — тонкий яркий ободок. */}
        <motion.div
          className="absolute inset-0 rounded-full"
          style={{
            boxShadow: active
              ? "0 0 2px 1px rgba(255,255,255,0.9), 0 0 22px 4px rgba(240,171,252,0.75), inset 0 0 14px 2px rgba(34,211,238,0.5)"
              : "0 0 2px 1px rgba(199,210,254,0.55), 0 0 16px 3px rgba(99,102,241,0.5), inset 0 0 12px 2px rgba(99,102,241,0.35)",
          }}
          animate={{ opacity: active ? [0.85, 1, 0.85] : [0.6, 0.8, 0.6] }}
          transition={{ duration: 2.2, repeat: Infinity, ease: "easeInOut" }}
        />
        {/* Горизонт событий — абсолютно тёмная сфера с тонким rim-light. */}
        <div
          className="rounded-full"
          style={{
            width: "88%",
            height: "88%",
            background:
              "radial-gradient(circle at 50% 42%, #0a0a12 0%, #050509 55%, #000 100%)",
            boxShadow: "inset 0 0 24px 6px rgba(0,0,0,0.9)",
          }}
        />
        {/* Метка состояния. */}
        <div className="absolute flex flex-col items-center">
          <span
            className="text-[11px] font-bold tracking-[0.25em]"
            style={{ color: active ? "#F5D0FE" : "#C7D2FE" }}
          >
            {busy ? "···" : active ? "ON" : "OFF"}
          </span>
        </div>
      </div>
    </motion.button>
  );
}

// Кольцо Эйнштейна: гравитационное линзирование с хроматической аберрацией.
// Три смещённые копии тонкого кольца (R/G/B) создают эффект расщепления света.
function LensingRing({ active, size }: { active: boolean; size: number }) {
  const ring = size * 0.5;
  const layer = (color: string, dx: number, dy: number, blur: number) => (
    <div
      className="absolute rounded-full"
      style={{
        width: ring,
        height: ring,
        left: `calc(50% - ${ring / 2}px + ${dx}px)`,
        top: `calc(50% - ${ring / 2}px + ${dy}px)`,
        border: `1.5px solid ${color}`,
        filter: `blur(${blur}px)`,
        mixBlendMode: "screen",
      }}
    />
  );
  const intensity = active ? 0.55 : 0.32;
  return (
    <motion.div
      className="pointer-events-none absolute inset-0 flex items-center justify-center"
      animate={{ rotate: 360 }}
      transition={{ duration: active ? 40 : 70, repeat: Infinity, ease: "linear" }}
      style={{ opacity: intensity }}
    >
      {layer("rgba(255,80,120,0.9)", -1.5, 0, 1.2)}
      {layer("rgba(120,255,180,0.8)", 0, 0, 0.8)}
      {layer("rgba(90,160,255,0.9)", 1.5, 0, 1.2)}
    </motion.div>
  );
}

// Наклонённый вращающийся аккреционный диск (два встречных кольца).
function AccretionDisk({ active, size }: { active: boolean; size: number }) {
  const idle =
    "conic-gradient(from 0deg, transparent 0%, rgba(99,102,241,0.0) 8%, rgba(99,102,241,0.55) 24%, rgba(34,211,238,0.85) 40%, rgba(139,92,246,0.5) 58%, transparent 74%, transparent 100%)";
  const hot =
    "conic-gradient(from 0deg, transparent 0%, rgba(34,211,238,0.2) 6%, rgba(240,171,252,0.9) 22%, rgba(253,230,138,1) 38%, rgba(34,211,238,0.9) 56%, rgba(139,92,246,0.6) 72%, transparent 88%)";
  return (
    <div
      className="pointer-events-none absolute"
      style={{
        width: size,
        height: size,
        transform: "perspective(560px) rotateX(70deg)",
        transformStyle: "preserve-3d",
      }}
    >
      {/* Внешнее кольцо. */}
      <div
        className={active ? "absolute inset-0 animate-spin-med" : "absolute inset-0 animate-spin-slow"}
        style={{
          borderRadius: "9999px",
          background: active ? hot : idle,
          maskImage: "radial-gradient(circle, transparent 30%, #000 40%, #000 70%, transparent 80%)",
          WebkitMaskImage:
            "radial-gradient(circle, transparent 30%, #000 40%, #000 70%, transparent 80%)",
          filter: active ? "blur(3px) brightness(1.25)" : "blur(3px)",
          opacity: active ? 0.95 : 0.7,
        }}
      />
      {/* Внутреннее встречное кольцо. */}
      <div
        className="absolute inset-0 animate-spin-rev"
        style={{
          borderRadius: "9999px",
          background: active ? hot : idle,
          maskImage: "radial-gradient(circle, transparent 24%, #000 32%, #000 52%, transparent 60%)",
          WebkitMaskImage:
            "radial-gradient(circle, transparent 24%, #000 32%, #000 52%, transparent 60%)",
          filter: active ? "blur(2px) brightness(1.3)" : "blur(2px)",
          opacity: active ? 0.9 : 0.6,
        }}
      />
    </div>
  );
}

// Canvas со спиральным падением частиц в ядро.
function InfallCanvas({ active, size }: { active: boolean; size: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const activeRef = useRef(active);
  activeRef.current = active;

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
    const horizon = size * 0.185;
    const outer = size * 0.52;

    type P = { a: number; r: number; av: number; rv: number; sz: number };
    const COUNT = 90;
    const spawn = (): P => ({
      a: Math.random() * Math.PI * 2,
      r: outer * (0.7 + Math.random() * 0.6),
      av: 0.006 + Math.random() * 0.01,
      rv: 0.25 + Math.random() * 0.5,
      sz: 0.6 + Math.random() * 1.6,
    });
    const parts: P[] = Array.from({ length: COUNT }, spawn);

    let raf = 0;
    const draw = () => {
      ctx.clearRect(0, 0, size, size);
      const hot = activeRef.current;
      const speedK = hot ? 1.9 : 1;

      for (const p of parts) {
        // Гравитация: чем ближе, тем быстрее (спираль внутрь).
        const pull = 1 + (outer - p.r) / outer;
        p.a += p.av * pull * speedK;
        p.r -= p.rv * pull * speedK * 0.5;

        if (p.r <= horizon) {
          Object.assign(p, spawn());
          continue;
        }

        const x = cx + Math.cos(p.a) * p.r;
        const y = cy + Math.sin(p.a) * p.r;
        const t = 1 - (p.r - horizon) / (outer - horizon); // 0 снаружи → 1 у ядра
        const alpha = Math.min(1, 0.15 + t * 0.9);

        // Цвет: снаружи индиго/циан, у горизонта — горячий.
        let color: string;
        if (hot) {
          color = t > 0.7 ? "253,230,138" : t > 0.4 ? "240,171,252" : "34,211,238";
        } else {
          color = t > 0.6 ? "199,210,254" : t > 0.3 ? "34,211,238" : "99,102,241";
        }

        ctx.beginPath();
        ctx.arc(x, y, p.sz * (0.7 + t), 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${color},${alpha})`;
        ctx.shadowBlur = hot ? 8 : 5;
        ctx.shadowColor = `rgba(${color},${alpha})`;
        ctx.fill();
      }
      ctx.shadowBlur = 0;
      raf = requestAnimationFrame(draw);
    };
    draw();
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
