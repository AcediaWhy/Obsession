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

// Живое ядро «Catnap»: закат в окне вагона. Тёплый янтарный диск-солнце низко в
// круге, медленно дышит — сонно, без нервного мерцания. Вверх лениво плывут
// золотые пылинки в закатном свете. Активация — солнце теплее и ярче; busy —
// дыхание чаще и мельче, «беспокойный сон».
export function CatnapCore({ active, busy = false, onClick, size = 240, paused = false }: Props) {
  // Ореолы гасим, когда окно скрыто ИЛИ это застывшее превью (paused):
  // framer-motion гоняет их на компоновщике (WAAPI) и сам на скрытие не реагирует.
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
      {/* Внешнее свечение-ореол — тёплый закатный свет из окна. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(255,184,112,0.30), rgba(226,120,52,0.15) 46%, transparent 72%)"
            : "radial-gradient(circle, rgba(236,158,92,0.18), transparent 68%)",
          filter: "blur(10px)",
        }}
        animate={
          renderOn
            ? {
                opacity: active ? [0.7, 1, 0.7] : [0.5, 0.68, 0.5],
                scale: active ? [1, 1.05, 1] : 1,
              }
            : { opacity: active ? 0.85 : 0.58, scale: 1 }
        }
        transition={
          renderOn
            ? { duration: active ? 4.2 : 6, repeat: Infinity, ease: "easeInOut" }
            : { duration: 0.3 }
        }
      />

      {/* Солнце и пылинки. */}
      <CatnapCanvas active={active} busy={busy} size={size} paused={paused} />

      {/* Тонкий ободок «стеклянной» кромки ядра. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(255,184,112,0.26), 0 0 22px 1px rgba(226,120,52,0.32)"
            : "inset 0 0 26px 2px rgba(236,158,92,0.20), 0 0 16px 1px rgba(196,104,58,0.22)",
          border: "1px solid rgba(255,255,255,0.06)",
        }}
        animate={renderOn ? { opacity: active ? [0.8, 1, 0.8] : [0.55, 0.75, 0.55] } : { opacity: active ? 0.9 : 0.65 }}
        transition={renderOn ? { duration: 3.4, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      {/* Метка состояния. */}
      <div className="pointer-events-none absolute flex flex-col items-center">
        <span
          className="text-2xs font-bold tracking-[0.32em]"
          style={{
            color: active ? "#FFE3BC" : "#F2CFA4",
            textShadow: active
              ? "0 0 14px rgba(255,184,112,0.85)"
              : "0 0 12px rgba(226,140,70,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas с солнцем и пылинками ────────────────────────────────────────────

type Mote = { x: number; y: number; vy: number; wob: number; wobPh: number; size: number; tw: number; twPh: number };

// Свечение пылинки, запечённое в спрайт по корзинам warm: холоднее золото в
// покое → почти белое тепло при активации. Яркость — в globalAlpha, радиус — в
// размер drawImage (см. glowSprite.ts).
const moteSprite = createSpriteCache(8, 32, (sctx, px, k) => {
  const r = px / 2;
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, `rgba(255,${Math.round(206 + k * 30)},${Math.round(140 + k * 50)},1)`);
  g.addColorStop(1, "rgba(255,190,120,0)");
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

function CatnapCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
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
    // Солнце чуть ниже центра — как низкое закатное в окне.
    const sunY = cy + size * 0.08;

    // Пылинки всплывают сквозь закатный свет и растворяются у верха.
    const spawn = (m: Mote) => {
      m.x = cx + (Math.random() - 0.5) * size * 0.5;
      m.y = cy + size * (0.16 + Math.random() * 0.18);
      m.vy = size * (0.03 + Math.random() * 0.05); // намного медленнее искр — сонно
      m.wob = size * (0.015 + Math.random() * 0.03);
      m.wobPh = Math.random() * Math.PI * 2;
      m.size = 0.8 + Math.random() * 1.6;
      m.tw = 0.5 + Math.random() * 0.9;
      m.twPh = Math.random() * Math.PI * 2;
    };
    const motes: Mote[] = Array.from({ length: 14 }, () => {
      const m: Mote = { x: 0, y: 0, vy: 0, wob: 0, wobPh: 0, size: 0, tw: 0, twPh: 0 };
      spawn(m);
      m.y = cy + (Math.random() - 0.5) * size * 0.5;
      return m;
    });

    let t = 0;
    // Сеем от текущего состояния — маунт при включённом щите сразу тёплый.
    let warm = stateRef.current.active ? 1 : 0;

    // Маска-феатеринг статична — строим один раз.
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

      // Сонное дыхание: один медленный синус; busy — чаще и мельче.
      const breath = loading
        ? 0.86 + 0.08 * Math.sin(t * 7)
        : 0.9 + 0.1 * Math.sin(t * 0.9);

      // Солнце: кремовый центр → янтарь → глубокий закатный край.
      const sunR = size * (0.22 + warm * 0.05) * breath;
      const sun = ctx.createRadialGradient(cx, sunY, 0, cx, sunY, sunR);
      const a = (0.5 + warm * 0.38) * (0.8 + 0.2 * breath);
      sun.addColorStop(0, `rgba(255,231,196,${a})`);
      sun.addColorStop(0.4, `rgba(255,184,112,${a * 0.8})`);
      sun.addColorStop(0.75, `rgba(226,120,52,${a * 0.45})`);
      sun.addColorStop(1, "rgba(150,58,26,0)");
      ctx.fillStyle = sun;
      ctx.beginPath();
      ctx.arc(cx, sunY, sunR, 0, Math.PI * 2);
      ctx.fill();

      // Полоса закатной дымки над солнцем — «горизонт» за стеклом. Мягкий
      // горизонтальный эллипс (сжатый по вертикали радиальный градиент). Раньше
      // тут был fillRect с градиентом ТОЛЬКО по вертикали: его прямые
      // левый/правый торцы (x = cx ± 0.36·size) попадали в непрозрачную зону
      // круговой маски-феатеринга и ТОРЧАЛИ жёсткой полосой за пределами круга
      // солнца — «шейдер съехал». Эллипс мягок со всех сторон и целиком внутри
      // диска, торцов нет.
      const hazeA = (0.10 + warm * 0.08) * breath;
      ctx.save();
      ctx.translate(cx, sunY + size * 0.02);
      ctx.scale(1, 0.32);
      const hazeR = size * 0.34;
      const haze = ctx.createRadialGradient(0, 0, 0, 0, 0, hazeR);
      haze.addColorStop(0, `rgba(240,150,80,${hazeA})`);
      haze.addColorStop(0.6, `rgba(232,132,64,${hazeA * 0.5})`);
      haze.addColorStop(1, "rgba(226,120,52,0)");
      ctx.fillStyle = haze;
      ctx.beginPath();
      ctx.arc(0, 0, hazeR, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      // Пылинки в свете — лениво вверх, мерцают медленно.
      for (const m of motes) {
        m.y -= m.vy * (1 + warm * 0.3) * dt;
        m.x += Math.sin(t * 0.8 + m.wobPh) * m.wob * dt;
        const top = cy - size * 0.34;
        if (m.y < top) spawn(m);
        const rise = Math.max(0, Math.min(1, (cy + size * 0.24 - m.y) / (size * 0.58)));
        const tw = 0.55 + 0.45 * Math.sin(t * m.tw + m.twPh);
        const ma = Math.min(1, (1 - rise) * tw * (0.5 + warm * 0.4));
        const r = m.size * (0.9 + warm * 0.25);
        const R = r * 3;
        ctx.globalAlpha = ma;
        ctx.drawImage(moteSprite(warm), m.x - R, m.y - R, R * 2, R * 2);
      }
      ctx.globalAlpha = 1;

      // Феатеринг в мягкий круг.
      ctx.globalCompositeOperation = "destination-in";
      ctx.fillStyle = mask;
      ctx.fillRect(0, 0, size, size);
      ctx.globalCompositeOperation = "source-over";
    };
    // paused из Настроек не передаётся — превью живут живыми (helper крутит цикл на
    // coreFps). paused НЕ в deps: ховер не пересоздаёт эффект. В трее цикл гасит гейт.
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
