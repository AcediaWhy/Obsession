import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { CORE_HERO_MIN_SIZE, coreFps, createRenderLoop, useRenderActive, type RenderLoop } from "../render";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
};

// Ядро темы «Rain»: поверхность воды. От падающих капель расходятся
// концентрические круги-рябь. В покое — редкие спокойные круги; при активации
// («гроза усиливается») капли падают чаще, рябь плотнее и ярче. Только вода и
// свет, никакой геометрии — форма читается органично в любом состоянии.
export function RainCore({ active, busy = false, onClick, size = 240, paused = false }: Props) {
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
      {/* Внешний ореол — дышит, холоднее в покое, ярко-голубой при грозе. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          inset: -size * 0.16,
          background: active
            ? "radial-gradient(circle, rgba(56,189,248,0.26), rgba(122,152,190,0.14) 46%, transparent 70%)"
            : "radial-gradient(circle, rgba(122,152,190,0.18), transparent 68%)",
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
            ? { duration: active ? 3.2 : 5, repeat: Infinity, ease: "easeInOut" }
            : { duration: 0.3 }
        }
      />

      {/* Поверхность воды с расходящейся рябью. */}
      <RippleCanvas active={active} busy={busy} size={size} paused={paused} />

      {/* Тонкий «стеклянный» ободок. */}
      <motion.div
        className="pointer-events-none absolute rounded-full"
        style={{
          inset: size * 0.06,
          boxShadow: active
            ? "inset 0 0 30px 2px rgba(56,189,248,0.26), 0 0 22px 1px rgba(56,189,248,0.32)"
            : "inset 0 0 26px 2px rgba(122,152,190,0.20), 0 0 16px 1px rgba(122,152,190,0.22)",
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
            color: active ? "#BAE6FD" : "#C3D3E4",
            textShadow: active
              ? "0 0 14px rgba(56,189,248,0.85)"
              : "0 0 12px rgba(122,152,190,0.7)",
          }}
        >
          {busy ? "···" : active ? "ON" : "OFF"}
        </span>
      </div>
    </motion.button>
  );
}

// ─── Canvas с расходящейся рябью ─────────────────────────────────────────────

type Ripple = {
  x: number; // центр удара, доля диаметра 0..1
  y: number;
  age: number; // возраст, сек
  life: number; // полное время жизни, сек
  strength: number; // 0..1 сила круга
};

function RippleCanvas({ active, busy, size, paused }: { active: boolean; busy: boolean; size: number; paused: boolean }) {
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

    const ripples: Ripple[] = [];
    let spawnAcc = 0; // накопитель порождения капель
    let t = 0;
    // 0..1 «разогрев»; сеем от текущего состояния — маунт при включённом щите
    // сразу тёплый, без прогрева на глазах.
    let warm = stateRef.current.active ? 1 : 0;

    // Маска-феатеринг статична (центр/радиусы от size) — строим один раз.
    const mask = ctx.createRadialGradient(size / 2, size / 2, size * 0.2, size / 2, size / 2, size * 0.5);
    mask.addColorStop(0, "rgba(0,0,0,1)");
    mask.addColorStop(0.74, "rgba(0,0,0,1)");
    mask.addColorStop(1, "rgba(0,0,0,0)");

    // Капли падают ближе к центру, но с разбросом — живая поверхность.
    const spawnRipple = (hot: number) => {
      const spread = 0.16 + Math.random() * 0.14;
      const ang = Math.random() * Math.PI * 2;
      const rad = Math.sqrt(Math.random()) * spread;
      ripples.push({
        x: 0.5 + Math.cos(ang) * rad,
        y: 0.5 + Math.sin(ang) * rad,
        age: 0,
        life: (2.4 - hot * 0.7) * (0.8 + Math.random() * 0.4),
        strength: 0.5 + Math.random() * 0.5,
      });
      if (ripples.length > 48) ripples.shift();
    };

    const draw = (dt: number) => {
      const { active: on, busy: loading } = stateRef.current;
      t += dt;
      warm += ((on ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.6));

      // Частота капель: спокойно в покое, ливень при грозе. При busy — нервный ритм.
      const rate = (1.1 + warm * 4.2) * (loading ? 1.6 : 1);
      spawnAcc += dt * rate;
      while (spawnAcc >= 1) {
        spawnAcc -= 1;
        spawnRipple(warm);
      }

      ctx.clearRect(0, 0, size, size);
      ctx.globalCompositeOperation = "lighter";

      // Матовая база воды — холодная, чуть светлеет при грозе.
      const baseA = 0.05 + warm * 0.06;
      const base = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size * 0.5);
      const bb = warm > 0.5 ? [125, 200, 245] : [122, 152, 190];
      base.addColorStop(0, `rgba(${bb[0]},${bb[1]},${bb[2]},${baseA})`);
      base.addColorStop(0.7, `rgba(${bb[0]},${bb[1]},${bb[2]},${baseA * 0.5})`);
      base.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = base;
      ctx.beginPath();
      ctx.arc(size / 2, size / 2, size * 0.5, 0, Math.PI * 2);
      ctx.fill();

      const maxR = size * 0.5;
      // Цвет ряби: серебристо-голубой в покое → ярко-голубой при грозе.
      const cr = Math.round(150 + (110 - 150) * warm);
      const cg = Math.round(180 + (215 - 180) * warm);
      const cb = Math.round(210 + (250 - 210) * warm);

      for (let i = ripples.length - 1; i >= 0; i--) {
        const rp = ripples[i];
        rp.age += dt;
        const p = rp.age / rp.life; // 0..1 прогресс
        if (p >= 1) {
          ripples.splice(i, 1);
          continue;
        }
        const cx = rp.x * size;
        const cy = rp.y * size;
        const r = p * maxR; // круг расходится от точки удара

        // Затухание: ярко у момента удара, гаснет по мере расширения.
        const fade = Math.pow(1 - p, 1.6);
        const a = rp.strength * fade * (0.5 + warm * 0.4);
        const lw = 0.6 + (1 - p) * 2.4;

        ctx.lineWidth = lw;
        ctx.strokeStyle = `rgba(${cr},${cg},${cb},${a})`;
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.stroke();

        // Второй, слабый внутренний круг — «двойная волна» настоящей ряби.
        const r2 = r - lw * 3;
        if (r2 > 1) {
          ctx.lineWidth = lw * 0.6;
          ctx.strokeStyle = `rgba(${cr},${cg},${cb},${a * 0.5})`;
          ctx.beginPath();
          ctx.arc(cx, cy, r2, 0, Math.PI * 2);
          ctx.stroke();
        }

        // Вспышка-всплеск в первый миг удара.
        if (p < 0.14) {
          const sa = rp.strength * (1 - p / 0.14) * (0.6 + warm * 0.4);
          const fl = ctx.createRadialGradient(cx, cy, 0, cx, cy, size * 0.05);
          fl.addColorStop(0, `rgba(${cr + 40},${cg + 20},${cb},${sa})`);
          fl.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = fl;
          ctx.beginPath();
          ctx.arc(cx, cy, size * 0.05, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      // Феатеринг в мягкий круг: свет только внутри радиальной маски.
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
  }, [active, busy, paused]);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute"
      style={{ width: size, height: size }}
    />
  );
}
