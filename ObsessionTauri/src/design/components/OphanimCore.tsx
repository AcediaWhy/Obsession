import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { CORE_HERO_MIN_SIZE, coreFps, createRenderLoop, useRenderActive, type RenderLoop } from "../render";
import { createSpriteCache } from "./glowSprite";
import { drawGreatEye } from "./ophanimEye";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  // Реактивная телеметрия щита (передаёт только hero на экране DPI; превью — нет,
  // поэтому дефолт false): scanning = идёт тест/автоподбор конфигов (Око «ищет»),
  // alarm = среди результатов теста есть провал (тревога, прищур, красный обод).
  scanning?: boolean;
  alarm?: boolean;
};

// «Офаним» — престольное колесо (Иез. 1): колёса-в-колёсах, вращающиеся сквозь
// друг друга под разными осями, усеянные глазами, в ореоле глориоли. В покое —
// тускло-золотое, глаза прикрыты; при активации разгоняется, теплеет в магенту,
// глаза раскрываются. Альтернативный hero к «Aurora», та же семантика состояний.
export function OphanimCore({ active, busy = false, onClick, size = 240, paused = false, scanning = false, alarm = false }: Props) {
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

      {/* Колёса-в-колёсах на canvas. */}
      <OphanimCanvas active={active} busy={busy} size={size} paused={paused} scanning={scanning} alarm={alarm} />

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
        animate={renderOn ? { opacity: active ? [0.8, 1, 0.8] : [0.55, 0.75, 0.55] } : { opacity: active ? 0.9 : 0.65 }}
        transition={renderOn ? { duration: 2.6, repeat: Infinity, ease: "easeInOut" } : { duration: 0.3 }}
      />

      {/* Без центральной ON/OFF-метки: она впечатывалась в радужку Великого Ока и
          сливалась с ним. Состояние несёт само Око (открыто+магента = вкл,
          дремотно-золото = выкл, рябь пробуждения на колёсах = busy), а на экране
          DPI под ядром уже есть текстовая подпись состояния. */}
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

// Свечение глаза, запечённое в спрайт по корзинам warm — свой кэш на колесо
// (у каждого свои cold/hot). В оригинале стопы (min(ea*1.4,1), ea, 0): яркость
// уходит в globalAlpha = min(ea*1.4, 1), в спрайте остаётся рампа (1, 1/1.4, 0).
// Раньше: до 40 createRadialGradient каждый кадр.
const eyeSprites = WHEELS.map((wh) =>
  createSpriteCache(9, 64, (sctx, px, k) => {
    const r = Math.round(wh.cold[0] + (wh.hot[0] - wh.cold[0]) * k);
    const g = Math.round(wh.cold[1] + (wh.hot[1] - wh.cold[1]) * k);
    const b = Math.round(wh.cold[2] + (wh.hot[2] - wh.cold[2]) * k);
    const R = px / 2;
    const grad = sctx.createRadialGradient(R, R, 0, R, R, R);
    grad.addColorStop(0, "rgba(255,255,255,1)");
    grad.addColorStop(0.4, `rgba(${r},${g},${b},${1 / 1.4})`);
    grad.addColorStop(1, "rgba(0,0,0,0)");
    sctx.fillStyle = grad;
    sctx.fillRect(0, 0, px, px);
  }),
);

// Великое Око вынесено в общий модуль ./ophanimEye (посадка в гнездо + дедуп с Field).

function OphanimCanvas({
  active,
  busy,
  size,
  paused,
  scanning,
  alarm,
}: {
  active: boolean;
  busy: boolean;
  size: number;
  paused: boolean;
  scanning: boolean;
  alarm: boolean;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef({ active, busy, scanning, alarm });
  stateRef.current = { active, busy, scanning, alarm };
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
    // 0..1 «разогрев»; сеем от текущего настроения — маунт при включённом щите
    // сразу тёплый, без прогрева на глазах.
    const s0 = stateRef.current;
    let warm = s0.active ? 1 : s0.scanning ? 0.6 : s0.busy ? 0.4 : 0;

    // Маска-феатеринг статична (центр/радиусы от size) — строим один раз.
    const mask = ctx.createRadialGradient(cx, cy, size * 0.2, cx, cy, size * 0.5);
    mask.addColorStop(0, "rgba(0,0,0,1)");
    mask.addColorStop(0.78, "rgba(0,0,0,1)");
    mask.addColorStop(1, "rgba(0,0,0,0)");

    // Точка на наклонённом эллипсе: локальные (rx·cosA, ry·sinA) → поворот на axis.
    const ellipsePt = (a: number, rx: number, ry: number, axis: number) => {
      const lx = rx * Math.cos(a);
      const ly = ry * Math.sin(a);
      const c = Math.cos(axis);
      const s = Math.sin(axis);
      return { x: cx + lx * c - ly * s, y: cy + lx * s + ly * c };
    };

    const draw = (dt: number) => {
      const { active: on, busy: loading, scanning: scan, alarm: alarmed } = stateRef.current;
      t += dt;
      // Настроение стражи: покой → пробуждение (busy) → скан (testing) → бдение (on).
      const warmTarget = on ? 1 : scan ? 0.6 : loading ? 0.4 : 0;
      warm += (warmTarget - warm) * (1 - Math.exp(-dt * 2.6));

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      const spinBoost = 1 + warm * 1.1; // при активации всё крутится быстрее

      // Око — престольная ступица, вокруг которой ВЬЮТСЯ колёса, а не крутятся
      // позади. Поэтому каждое колесо рисуется в ДВА захода с сортировкой по
      // глубине 3D-наклона: дальняя половина (уходит от зрителя) — ПОД Оком,
      // ближняя (идёт к зрителю) — ПОВЕРХ. Раскол по большой оси эллипса (a=0,π)
      // на радиусе rx — заведомо снаружи Ока. Глубина: z ∝ sin(a)·cosP.
      const eyeR = size * 0.19;
      const clearR = eyeR * 1.15; // «чистая зона» лица Ока: глаза-спрайты внутри — всегда позади

      const paintWheelLayer = (near: boolean) => {
        for (const [wi, wh] of WHEELS.entries()) {
          const rx = wh.r * size;
          const phase = t * wh.tiltSpeed * spinBoost + wh.tiltPhase;
          const cosP = Math.cos(phase); // знак = глубина наклона (±к зрителю)
          // ry «переворачивается» в 3D: от ребра (~0.12) до полного круга.
          const ry = rx * (0.12 + 0.88 * Math.abs(Math.sin(phase)));

          // Цвет обода: лерп cold→hot по warm.
          const [cr, cg, cb] = wh.cold;
          const [hr, hg, hb] = wh.hot;
          const r = Math.round(cr + (hr - cr) * warm);
          const g = Math.round(cg + (hg - cg) * warm);
          const b = Math.round(cb + (hb - cb) * warm);
          const rimA = 0.1 + warm * 0.16;

          // Половина обода по глубине: при cosP≥0 ближе верхняя дуга a∈(0,π).
          const drawUpper = near === cosP >= 0;
          const i0 = drawUpper ? 0 : 32;
          const i1 = drawUpper ? 32 : 64;
          const traceRim = () => {
            ctx.beginPath();
            for (let i = i0; i <= i1; i++) {
              const a = (i / 64) * Math.PI * 2;
              const p = ellipsePt(a, rx, ry, wh.axis);
              i === i0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y);
            }
          };
          // Обод: два штриха — широкий тусклый (свечение) + узкий яркий.
          ctx.strokeStyle = `rgba(${r},${g},${b},${rimA * 0.5})`;
          ctx.lineWidth = wh.lw * size * 3;
          traceRim();
          ctx.stroke();
          ctx.strokeStyle = `rgba(${Math.min(r + 30, 255)},${Math.min(g + 30, 255)},${b},${rimA})`;
          ctx.lineWidth = wh.lw * size;
          traceRim();
          ctx.stroke();

          // Глаза по ободу — бегут, раскрываются с warm, «моргают». Та же глубинная
          // сортировка; внутри чистой зоны — только задний проход (Око перекрывает,
          // иначе яркий глаз поверх зрачка читался бы как мусор).
          const openBase = 0.18 + warm * 0.82;
          for (let e = 0; e < wh.eyes; e++) {
            const a = (e / wh.eyes) * Math.PI * 2 + t * wh.spin * spinBoost;
            const p = ellipsePt(a, rx, ry, wh.axis);
            const dist = Math.hypot(p.x - cx, p.y - cy);
            if (dist < clearR) {
              if (near) continue;
            } else if ((Math.sin(a) * cosP >= 0) !== near) {
              continue;
            }
            // Глаза на «дальней» стороне (верх наклона) тусклее → ощущение объёма.
            const depth = 0.55 + 0.45 * (0.5 + 0.5 * Math.sin(a + wh.axis));
            // Индивидуальное моргание + бегущая вспышка при busy.
            const blink = 0.5 + 0.5 * Math.sin(t * 2.2 + e * 1.3 + wh.tiltPhase);
            const chase = loading ? Math.max(0, Math.sin(t * 5 - e * (6.283 / wh.eyes))) : 0;
            const open = Math.min(1, openBase * (0.6 + 0.4 * blink) + chase * 0.6);
            const rad = wh.lw * size * (1.6 + open * 1.6);

            const ea = (0.22 + warm * 0.5) * depth * (0.4 + 0.6 * open);
            const R = rad * 2.4;
            ctx.globalAlpha = Math.min(ea * 1.4, 1);
            ctx.drawImage(eyeSprites[wi](warm), p.x - R, p.y - R, R * 2, R * 2);
          }
          ctx.globalAlpha = 1;
        }
      };

      // Дальние половины колёс — уходят ПОД Око.
      paintWheelLayer(false);

      // ─── Великое Око в центре: престольная ступица, обвитая колёсами. ───
      drawGreatEye(ctx, cx, cy, eyeR, {
        t,
        warm,
        scanning: scan,
        alarmed,
        bloomAlpha: 0.3 + warm * 0.42,
      });

      // Ближние половины — проходят ПОВЕРХ Ока: вот оно «вокруг», а не «позади».
      paintWheelLayer(true);

      // Феатеринг в мягкий круг: гасим всё за пределами радиального маска.
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

  // Застывшее превью перерисовываем при смене active/busy (выбор темы меняет
  // цвет замершего кадра) — раньше это давал полный ремоунт эффекта.
  useEffect(() => {
    // Живой цикл: no-op; замерший (paused/reduce-motion) — дорисовать кадр.
    loopRef.current?.invalidate();
  }, [active, busy, scanning, alarm, paused]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size }}
    />
  );
}
