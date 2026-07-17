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
import { createSpriteCache } from "./glowSprite";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
};

// Живое ядро «Aurora»: текучие световые шторы (canvas), собранные в мягкий
// светящийся круг. Никакой геометрии дисков — только колышущийся свет, поэтому
// форма читается органично в любом состоянии. Центральный элемент айдентики.
export function AuroraCore({ active, busy = false, onClick, size = 240, paused = false }: Props) {
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
        animate={
          renderOn
            ? {
                opacity: active ? [0.7, 1, 0.7] : [0.5, 0.7, 0.5],
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

      {/* Aurora-шторы внутри мягкого круга. */}
      <AuroraCanvas active={active} busy={busy} size={size} paused={paused} />

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
        animate={renderOn ? { opacity: active ? [0.8, 1, 0.8] : [0.55, 0.75, 0.55] } : { opacity: active ? 0.9 : 0.65 }}
        transition={renderOn ? { duration: 2.6, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      {/* Метка состояния. */}
      <div className="pointer-events-none absolute flex flex-col items-center">
        <span
          className="text-2xs font-bold tracking-[0.32em]"
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
  rays: number; // число вертикальных лучей-полос внутри шторы
  cold: [number, number, number]; // цвет в покое (r,g,b) — индиго/циан/бирюза
  hot: [number, number, number]; // цвет при активации
};

const RIBBONS: Ribbon[] = [
  { x: 0.30, width: 0.20, amp: 0.05, freq: 0.9, speed: 0.34, phase: 0.0, rays: 5, cold: [99, 102, 241], hot: [139, 92, 246] },
  { x: 0.46, width: 0.24, amp: 0.06, freq: 1.2, speed: 0.46, phase: 1.7, rays: 6, cold: [45, 212, 191], hot: [240, 171, 252] },
  { x: 0.60, width: 0.18, amp: 0.055, freq: 1.0, speed: 0.30, phase: 3.1, rays: 5, cold: [34, 211, 238], hot: [253, 230, 138] },
  { x: 0.70, width: 0.16, amp: 0.05, freq: 1.4, speed: 0.52, phase: 4.6, rays: 4, cold: [16, 185, 129], hot: [34, 211, 238] },
];

// Искорка-звезда позади лент (мягкое ядро + крест-блик), запечена в спрайт по
// корзинам яркости — тот же язык, что звёзды фона (см. AuroraField).
const coreGlint = createSpriteCache(5, 40, (sctx, px, k) => {
  const R = px / 2;
  const b = 0.55 + k * 0.45;
  const core = sctx.createRadialGradient(R, R, 0, R, R, R * 0.5);
  core.addColorStop(0, `rgba(255,255,255,${b})`);
  core.addColorStop(0.5, `rgba(202,222,255,${b * 0.4})`);
  core.addColorStop(1, "rgba(202,222,255,0)");
  sctx.fillStyle = core;
  sctx.fillRect(0, 0, px, px);
  const spike = (horizontal: boolean) => {
    const g = horizontal
      ? sctx.createLinearGradient(0, R, px, R)
      : sctx.createLinearGradient(R, 0, R, px);
    g.addColorStop(0, "rgba(220,235,255,0)");
    g.addColorStop(0.5, `rgba(235,244,255,${b * 0.7})`);
    g.addColorStop(1, "rgba(220,235,255,0)");
    sctx.fillStyle = g;
    if (horizontal) sctx.fillRect(0, R - 1, px, 2);
    else sctx.fillRect(R - 1, 0, 2, px);
  };
  spike(true);
  spike(false);
});

// Фиксированные искры внутри круга (позиции детерминированы — стабильны между
// перемонтированиями). Доли размера от центра; держим внутри маски (r<0.42).
const SPARKS = [
  { dx: -0.24, dy: -0.2, r: 1.6, spd: 0.7, ph: 0.0 },
  { dx: 0.26, dy: -0.14, r: 1.3, spd: 1.1, ph: 1.9 },
  { dx: -0.12, dy: 0.26, r: 1.5, spd: 0.9, ph: 3.4 },
  { dx: 0.2, dy: 0.22, r: 1.2, spd: 1.3, ph: 5.0 },
];

function AuroraCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
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
    let mask: CanvasGradient;
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
      mask = ctx.createRadialGradient(size / 2, size / 2, size * 0.2, size / 2, size / 2, size * 0.5);
      mask.addColorStop(0, "rgba(0,0,0,1)");
      mask.addColorStop(0.72, "rgba(0,0,0,1)");
      mask.addColorStop(1, "rgba(0,0,0,0)");
    };
    resizeBacking("high");

    let t = 0;
    // 0..1 «разогрев»; сеем от текущего состояния — маунт при включённом щите
    // (смена темы/экрана) сразу тёплый, без прогрева на глазах.
    let warm = stateRef.current.active ? 1 : 0;


    const draw = (dt: number) => {
      const { active: on, busy: loading } = stateRef.current;
      // Кадронезависимо: одинаковая скорость на 60/120/144 Гц.
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      // Искры-звёзды позади лент — мягко мерцают.
      for (const sp of SPARKS) {
        const tw = 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * sp.spd + sp.ph));
        const a = tw * (0.5 + warm * 0.4);
        const R = sp.r * 5;
        ctx.globalAlpha = a;
        ctx.drawImage(coreGlint(a), size / 2 + sp.dx * size - R, size / 2 + sp.dy * size - R, R * 2, R * 2);
      }
      ctx.globalAlpha = 1;

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

        // Лучи-полосы внутри ленты — «расчёсанная» текстура сияния (как в фоне).
        const rr = Math.min(r + 40, 255);
        const rg = Math.min(g + 40, 255);
        const rayGrad = ctx.createLinearGradient(0, 0, 0, size);
        rayGrad.addColorStop(0.0, `rgba(${rr},${rg},${b},0)`);
        rayGrad.addColorStop(0.4, `rgba(${rr},${rg},${b},1)`);
        rayGrad.addColorStop(1.0, `rgba(${rr},${rg},${b},0)`);
        ctx.strokeStyle = rayGrad;
        ctx.lineWidth = 1;
        for (let i = 0; i < rb.rays; i++) {
          const off = (i / (rb.rays - 1) - 0.5) * rb.width * size * 0.85;
          const shimmer = 0.1 + 0.18 * (0.5 + 0.5 * Math.sin(t * (1.4 + rb.speed * 3) + i * 1.7 + rb.phase));
          ctx.globalAlpha = shimmer * (0.6 + warm * 0.8) * flick;
          ctx.beginPath();
          for (let y = 0; y <= size; y += step) {
            const x = centerAt(y) + off;
            y === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
          }
          ctx.stroke();
        }
        ctx.globalAlpha = 1;
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
      ctx.fillStyle = mask;
      ctx.fillRect(0, 0, size, size);
      ctx.globalCompositeOperation = "source-over";
    };
    // paused НЕ в deps: ховер не пересоздаёт эффект. В трее цикл гасит общий scheduler.
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

  // Застывший кадр (превью paused ИЛИ стоп-кадр reduce-motion) перерисовываем
  // при смене active/busy — замерший кадр следует состоянию. На живом цикле
  // invalidate — no-op.
  useEffect(() => {
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
