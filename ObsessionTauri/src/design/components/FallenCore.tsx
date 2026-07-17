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

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
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

export function FallenCore({ active, busy = false, onClick, size = 240, paused = false }: Props) {
  // Ореолы гасим, когда окно скрыто ИЛИ это застывшее превью невыбранной темы
  // (paused): framer-motion гоняет их на компоновщике (WAAPI) и сам на скрытие не
  // реагирует — жёг бы CPU в трее и в Настройках (6 превью × 2 ореола).
  const renderOn = useRenderActive() && !paused;
  return (
    <motion.button
      onClick={onClick}
      disabled={busy}
      whileHover={{ scale: 1.03 }}
      whileTap={{ scale: 0.97 }}
      className="no-drag relative flex items-center justify-center disabled:cursor-wait"
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
        animate={renderOn ? { opacity: active ? [0.6, 0.9, 0.6] : [0.4, 0.58, 0.4] } : { opacity: active ? 0.75 : 0.5 }}
        transition={renderOn ? { duration: active ? 4.2 : 6, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      <SoulCanvas active={active} busy={busy} size={size} paused={paused} />

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
        animate={renderOn ? { opacity: active ? [0.7, 0.9, 0.7] : [0.5, 0.66, 0.5] } : { opacity: active ? 0.8 : 0.58 }}
        transition={renderOn ? { duration: 3.2, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      {/* Метка состояния под сердцем. */}
      <div className="pointer-events-none absolute flex flex-col items-center" style={{ marginTop: size * 0.34 }}>
        <span
          className="text-2xs font-bold tracking-[0.32em]"
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

function SoulCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
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
      ctx.imageSmoothingEnabled = false;
    };
    resizeBacking("high");

    const cx = size / 2;
    const cy = size / 2;

    let t = 0;
    // 0..1 «разогрев»; сеем от текущего состояния — маунт при включённом щите
    // сразу тёплый, без прогрева на глазах.
    let warm = stateRef.current.active ? 1 : 0;

    // Сердцебиение: два толчка (lub-dub) и пауза, свёрнутые в фазу 0..1.
    const heartbeat = (u: number) => {
      const g = (c: number, s: number) => Math.exp(-((u - c) * (u - c)) / (s * s));
      return g(0.0, 0.055) + g(0.17, 0.06) * 0.62;
    };

    const draw = (dt: number) => {
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
    // paused НЕ в deps: ховер не пересоздаёт эффект. В трее цикл гасит общий scheduler.
    // Кламп dt — дефолтный 0.1: прежний 0.05 при капе 20 fps постоянно
    // срезал реальный интервал и замедлял сердцебиение на ~9%.
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

  // Застывшее превью перерисовываем при смене active/busy (выбор темы меняет
  // цвет замершего кадра) — раньше это давал полный ремоунт эффекта.
  useEffect(() => {
    // Живой цикл: no-op; замерший (paused/reduce-motion) — дорисовать кадр.
    loopRef.current?.invalidate();
  }, [active, busy, paused]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size, imageRendering: "pixelated" }}
    />
  );
}
