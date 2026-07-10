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

// Живое ядро «Fireflies»: рой светлячков кружит по мягким орбитам и складывается
// в светящееся кольцо-«глаз» с тёплым зрачком в центре. Каждый огонёк тихо
// перемигивается. Активация — теплее/ярче, орбиты чуть туже, «дыхание» быстрее;
// busy — нервное частое мигание роя.
export function FirefliesCore({ active, busy = false, onClick, size = 240, paused = false }: Props) {
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
      {/* Внешнее свечение-ореол — дышит, теплеет при активации. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(214,224,120,0.30), rgba(180,168,96,0.14) 46%, transparent 70%)"
            : "radial-gradient(circle, rgba(150,170,110,0.18), transparent 68%)",
          filter: "blur(10px)",
        }}
        animate={
          renderOn
            ? {
                opacity: active ? [0.7, 1, 0.7] : [0.5, 0.72, 0.5],
                scale: active ? [1, 1.05, 1] : 1,
              }
            : { opacity: active ? 0.85 : 0.6, scale: 1 }
        }
        transition={
          renderOn
            ? { duration: active ? 3.4 : 5, repeat: Infinity, ease: "easeInOut" }
            : { duration: 0.3 }
        }
      />

      {/* Рой светлячков. */}
      <FirefliesCanvas active={active} busy={busy} size={size} paused={paused} />

      {/* Тонкий ободок «стеклянной» кромки ядра. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(214,224,120,0.26), 0 0 22px 1px rgba(180,168,96,0.32)"
            : "inset 0 0 26px 2px rgba(150,170,110,0.20), 0 0 16px 1px rgba(120,140,110,0.22)",
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
            color: active ? "#E6EE9C" : "#CBD5B0",
            textShadow: active
              ? "0 0 14px rgba(214,224,120,0.8)"
              : "0 0 12px rgba(150,170,110,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas с роем светлячков ────────────────────────────────────────────────

type Fly = {
  orbit: number; // радиус орбиты, доля size/2
  ang: number; // текущий угол
  spd: number; // угловая скорость
  wob: number; // амплитуда «дыхания» радиуса
  wobPh: number;
  size: number; // радиус свечения, px
  base: number; // базовая яркость
  blinkSpd: number;
  blinkPh: number;
};

// Свечение светлячка, запечённое в спрайт по корзинам warm (цвет зависит только
// от warm): яркость уходит в globalAlpha, радиус — в размер drawImage. Раньше:
// 22 createRadialGradient каждый кадр.
const flySprite = createSpriteCache(9, 64, (sctx, px, k) => {
  const r = px / 2;
  const cr = Math.round(214 + k * 28);
  const cg = Math.round(228 - k * 8);
  const cb = Math.round(128 - k * 44);
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, `rgba(${cr},${cg},${cb},1)`);
  g.addColorStop(0.4, `rgba(${cr},${cg - 20},${cb},0.45)`);
  g.addColorStop(1, `rgba(${cr},${cg},${cb},0)`);
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

const FLIES: Fly[] = Array.from({ length: 22 }, (_, i) => {
  const ring = 0.52 + (i % 3) * 0.13; // три рыхлых кольца
  return {
    orbit: ring + (Math.random() - 0.5) * 0.08,
    ang: (i / 22) * Math.PI * 2 + Math.random() * 0.4,
    spd: (0.18 + Math.random() * 0.22) * (i % 2 ? 1 : -1), // встречные потоки
    wob: 0.04 + Math.random() * 0.05,
    wobPh: Math.random() * Math.PI * 2,
    size: 6 + Math.random() * 7,
    base: 0.4 + Math.random() * 0.4,
    blinkSpd: 0.8 + Math.random() * 1.6,
    blinkPh: Math.random() * Math.PI * 2,
  };
});

function FirefliesCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
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
    let t = 0;
    let warm = 0;

    // Маска-феатеринг статична (центр/радиусы от size) — строим один раз.
    const mask = ctx.createRadialGradient(cx, cy, size * 0.2, cx, cy, size * 0.5);
    mask.addColorStop(0, "rgba(0,0,0,1)");
    mask.addColorStop(0.74, "rgba(0,0,0,1)");
    mask.addColorStop(1, "rgba(0,0,0,0)");

    const drawFly = (x: number, y: number, r: number, a: number, warmv: number) => {
      ctx.globalAlpha = a;
      ctx.drawImage(flySprite(warmv), x - r, y - r, r * 2, r * 2);
    };

    const draw = (dt: number) => {
      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      // Тёплый зрачок-свечение в центре — «дышит».
      const breathe = 0.5 + Math.sin(t * (1.4 + warm * 0.6)) * 0.5;
      const pupilR = size * (0.14 + warm * 0.04 + breathe * 0.02);
      const pupil = ctx.createRadialGradient(cx, cy, 0, cx, cy, pupilR);
      const pa = 0.3 + warm * 0.35;
      pupil.addColorStop(0, `rgba(${240},${238},${200},${pa})`);
      pupil.addColorStop(0.5, warm > 0.5 ? `rgba(214,196,110,${pa * 0.5})` : `rgba(150,170,110,${pa * 0.5})`);
      pupil.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = pupil;
      ctx.beginPath();
      ctx.arc(cx, cy, pupilR, 0, Math.PI * 2);
      ctx.fill();

      // Рой на орбитах: при активации кольцо туже (radius × 0.9), busy — рваное мигание.
      const tighten = 1 - warm * 0.1;
      for (const f of FLIES) {
        f.ang += f.spd * (1 + warm * 0.5) * dt;
        const rr = (f.orbit + Math.sin(t * 0.8 + f.wobPh) * f.wob) * (size / 2) * tighten;
        const x = cx + Math.cos(f.ang) * rr;
        const y = cy + Math.sin(f.ang) * rr;
        let blink = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * f.blinkSpd + f.blinkPh));
        if (loading) blink = 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * (f.blinkSpd + 6) + f.blinkPh));
        const a = Math.min(1, f.base * blink * (0.85 + warm * 0.5));
        drawFly(x, y, f.size, a, warm);
      }
      ctx.globalAlpha = 1;

      // Феатеринг в мягкий круг (как в Aurora).
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
