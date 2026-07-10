import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { CORE_HERO_MIN_SIZE, coreFps, createRenderLoop, useRenderActive, type RenderLoop } from "../render";
import { createSpriteCache } from "./glowSprite";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
};

// Живое ядро «Hearth»: тёплый тлеющий уголёк. Пульсирующее янтарное ядро с
// мерцающей — как живое пламя — яркостью, от которого вверх лениво слетают искры.
// Активация — жар ярче и «выше», теплее; busy — рваное, нервное мерцание.
export function HearthCore({ active, busy = false, onClick, size = 240, paused = false }: Props) {
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
      data-snow="round"
      className="no-drag snow-surface relative flex items-center justify-center disabled:cursor-wait"
      style={{ width: size, height: size }}
    >
      {/* Внешнее свечение-ореол — дышит, разгорается при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(255,164,74,0.32), rgba(226,120,52,0.16) 46%, transparent 72%)"
            : "radial-gradient(circle, rgba(226,132,58,0.20), transparent 68%)",
          filter: "blur(10px)",
        }}
        animate={
          renderOn
            ? {
                opacity: active ? [0.72, 1, 0.72] : [0.5, 0.72, 0.5],
                scale: active ? [1, 1.06, 1] : 1,
              }
            : { opacity: active ? 0.86 : 0.6, scale: 1 }
        }
        transition={
          renderOn
            ? { duration: active ? 3.2 : 5, repeat: Infinity, ease: "easeInOut" }
            : { duration: 0.3 }
        }
      />

      {/* Уголёк и искры. */}
      <HearthCanvas active={active} busy={busy} size={size} paused={paused} />

      {/* Тонкий ободок «стеклянной» кромки ядра. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(255,164,74,0.28), 0 0 22px 1px rgba(226,120,52,0.34)"
            : "inset 0 0 26px 2px rgba(226,132,58,0.22), 0 0 16px 1px rgba(196,92,64,0.24)",
          border: "1px solid rgba(255,255,255,0.06)",
        }}
        animate={renderOn ? { opacity: active ? [0.8, 1, 0.8] : [0.55, 0.75, 0.55] } : { opacity: active ? 0.9 : 0.65 }}
        transition={renderOn ? { duration: 2.6, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      {/* Метка состояния. */}
      <div className="pointer-events-none absolute flex flex-col items-center">
        <span
          className="text-[11px] font-bold tracking-[0.32em]"
          style={{
            color: active ? "#FFD9A0" : "#F0C89A",
            textShadow: active
              ? "0 0 14px rgba(255,164,74,0.85)"
              : "0 0 12px rgba(226,132,58,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas с угольком и искрами ─────────────────────────────────────────────

type Spark = { x: number; y: number; vy: number; wob: number; wobPh: number; size: number; heat: number };

// Свечение искры, запечённое в спрайт по корзинам rise (0 у низа → 1 у верха):
// яркость уходит в globalAlpha, радиус — в размер drawImage. Раньше:
// 16 createRadialGradient каждый кадр.
const sparkSprite = createSpriteCache(8, 32, (sctx, px, k) => {
  const r = px / 2;
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, `rgba(255,${Math.round(180 + k * 50)},${Math.round(90 + k * 40)},1)`);
  g.addColorStop(1, "rgba(255,170,90,0)");
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

function HearthCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
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

    // Превью (size<160) рисуем в 1×: мягким свечениям хватает, а пикселей — и
    // работы композитору — вчетверо меньше. Герой остаётся чётким (dpr до 2).
    const dpr = size >= CORE_HERO_MIN_SIZE ? Math.min(window.devicePixelRatio || 1, 2) : 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    const cx = size / 2;
    const cy = size / 2;

    // Искры рождаются у нижней кромки уголька и всплывают внутри диска.
    const spawn = (s: Spark) => {
      s.x = cx + (Math.random() - 0.5) * size * 0.28;
      s.y = cy + size * (0.1 + Math.random() * 0.12);
      s.vy = size * (0.18 + Math.random() * 0.26);
      s.wob = size * (0.02 + Math.random() * 0.05);
      s.wobPh = Math.random() * Math.PI * 2;
      s.size = 1 + Math.random() * 2.2;
      s.heat = 0.6 + Math.random() * 0.4;
    };
    const sparks: Spark[] = Array.from({ length: 16 }, () => {
      const s: Spark = { x: 0, y: 0, vy: 0, wob: 0, wobPh: 0, size: 0, heat: 0 };
      spawn(s);
      s.y = cy + (Math.random() - 0.5) * size * 0.4;
      return s;
    });

    let t = 0;
    let warm = 0;

    // Маска-феатеринг статична (центр/радиусы от size) — строим один раз.
    const mask = ctx.createRadialGradient(cx, cy, size * 0.2, cx, cy, size * 0.5);
    mask.addColorStop(0, "rgba(0,0,0,1)");
    mask.addColorStop(0.72, "rgba(0,0,0,1)");
    mask.addColorStop(1, "rgba(0,0,0,0)");

    const draw = (dt: number) => {
      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      // Мерцание пламени: сумма синусов; busy добавляет рваный высокочастотный джиттер.
      let flick =
        0.66 + 0.16 * Math.sin(t * 5.6) + 0.1 * Math.sin(t * 12.3 + 1.1) + 0.08 * Math.sin(t * 20.5 + 2.2);
      if (loading) flick *= 0.6 + 0.4 * Math.sin(t * 26);
      flick = Math.max(0.32, flick);

      // Ядро-уголёк: горячий жёлто-белый центр → янтарь → тёмно-красный край.
      const coreR = size * (0.2 + warm * 0.05) * (0.9 + flick * 0.16);
      const core = ctx.createRadialGradient(cx, cy + size * 0.02, 0, cx, cy + size * 0.02, coreR);
      const a = (0.5 + warm * 0.4) * flick;
      core.addColorStop(0, `rgba(255,236,190,${a})`);
      core.addColorStop(0.35, `rgba(255,168,80,${a * 0.85})`);
      core.addColorStop(0.7, `rgba(214,92,44,${a * 0.5})`);
      core.addColorStop(1, "rgba(120,36,18,0)");
      ctx.fillStyle = core;
      ctx.beginPath();
      ctx.arc(cx, cy + size * 0.02, coreR, 0, Math.PI * 2);
      ctx.fill();

      // Искры вверх.
      for (const s of sparks) {
        s.y -= s.vy * (1 + warm * 0.4) * dt;
        s.x += Math.sin(t * 2 + s.wobPh) * s.wob;
        const top = cy - size * 0.32;
        if (s.y < top) spawn(s);
        const rise = Math.max(0, Math.min(1, (cy + size * 0.2 - s.y) / (size * 0.5)));
        const sa = Math.min(1, s.heat * (1 - rise) * flick * (0.7 + warm * 0.5));
        const r = s.size * (0.9 + warm * 0.3);
        const R = r * 3;
        ctx.globalAlpha = sa;
        ctx.drawImage(sparkSprite(rise), s.x - R, s.y - R, R * 2, R * 2);
      }
      ctx.globalAlpha = 1;

      // Феатеринг в мягкий круг.
      ctx.globalCompositeOperation = "destination-in";
      ctx.fillStyle = mask;
      ctx.fillRect(0, 0, size, size);
      ctx.globalCompositeOperation = "source-over";
    };
    // Превью в Настройках монтируются уже застывшими (paused): хелпер рисует
    // один кадр и глушит rAF. paused НЕ в deps — ховер не пересоздаёт эффект.
    const loop = createRenderLoop(draw, { fps: coreFps(size), paused: pausedRef.current });
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
    if (paused) loopRef.current?.invalidate();
  }, [active, busy, paused]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size }}
    />
  );
}
