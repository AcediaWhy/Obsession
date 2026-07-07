import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";

// Фон темы «Fallen Down»: тихий сумеречный дождь. Косые струи, лёгкий туман,
// редкая рябь у нижней кромки. В покое — холодный сланец; при активном обходе
// пространство чуть теплеет и дождь стихает (спокойствие «под защитой»).
export function FallenField() {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;
  const hotRef = useRef(hot);
  hotRef.current = hot;

  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const Q = 0.6;
    let w = 0;
    let h = 0;
    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * Q));
      canvas.height = Math.max(1, Math.round(h * Q));
      ctx.setTransform(Q, 0, 0, Q, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    // Струи дождя: слои с разной скоростью для ощущения глубины.
    type Streak = { x: number; y: number; len: number; speed: number; a: number };
    const ANGLE = 0.18;
    let streaks: Streak[] = [];
    const seedStreaks = () => {
      const count = Math.round((w * h) / 26000); // плотность ~ площади
      streaks = Array.from({ length: Math.max(40, Math.min(160, count)) }, () => ({
        x: Math.random() * (w + h * ANGLE) - h * ANGLE,
        y: Math.random() * h,
        len: 10 + Math.random() * 22,
        speed: 260 + Math.random() * 340,
        a: 0.05 + Math.random() * 0.12,
      }));
    };
    seedStreaks();

    // Рябь у «земли» (нижняя кромка).
    type Ripple = { x: number; y: number; r: number; alpha: number };
    const ripples: Ripple[] = [];
    let rippleAcc = 0;

    let t = 0;
    let warm = 0;
    let raf = 0;
    let last = 0;
    const FRAME = 1000 / 30;

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0;
        return;
      }
      if (now - last < FRAME) return;
      const dt = last ? Math.min((now - last) / 1000, 0.08) : 0.033;
      last = now;
      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, w, h);

      const speedK = 1 - warm * 0.3;

      // Дождь.
      ctx.strokeStyle = `rgba(150,175,210,${1})`;
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (const s of streaks) {
        s.y += s.speed * speedK * dt;
        s.x += s.speed * speedK * dt * ANGLE;
        if (s.y - s.len > h) {
          s.y = -Math.random() * h * 0.3;
          s.x = Math.random() * (w + h * ANGLE) - h * ANGLE;
        }
      }
      // Рисуем per-streak, чтобы у каждого была своя прозрачность.
      for (const s of streaks) {
        ctx.strokeStyle = `rgba(150,175,210,${s.a * (1 - warm * 0.35)})`;
        ctx.beginPath();
        ctx.moveTo(s.x, s.y);
        ctx.lineTo(s.x - s.len * ANGLE, s.y - s.len);
        ctx.stroke();
      }

      // Периодически рождаем рябь у нижней кромки.
      rippleAcc += dt;
      const interval = 0.12 + warm * 0.2; // при активации реже
      while (rippleAcc > interval) {
        rippleAcc -= interval;
        ripples.push({
          x: Math.random() * w,
          y: h - 6 - Math.random() * h * 0.06,
          r: 1,
          alpha: 0.16 - warm * 0.06,
        });
      }
      ctx.globalCompositeOperation = "lighter";
      for (let i = ripples.length - 1; i >= 0; i--) {
        const rp = ripples[i];
        rp.r += 46 * dt;
        rp.alpha -= dt * 0.22;
        if (rp.alpha <= 0 || rp.r > 46) {
          ripples.splice(i, 1);
          continue;
        }
        ctx.strokeStyle = `rgba(170,195,225,${Math.max(rp.alpha, 0)})`;
        ctx.beginPath();
        ctx.ellipse(rp.x, rp.y, rp.r, rp.r * 0.3, 0, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.globalCompositeOperation = "source-over";
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Сумеречный градиент глубины — холодный сланец. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_50%_18%,#141a28_0%,#0b0f18_50%,#05070c_100%)]" />
      {/* Мягкие холодные пятна тумана. */}
      <div
        className="absolute left-[12%] top-[8%] h-[440px] w-[440px] rounded-full opacity-30"
        style={{ background: "radial-gradient(circle, rgba(90,110,150,0.5), transparent 66%)", filter: "blur(100px)" }}
      />
      <div
        className="absolute right-[10%] top-[24%] h-[380px] w-[380px] rounded-full opacity-25"
        style={{ background: "radial-gradient(circle, rgba(120,130,170,0.4), transparent 66%)", filter: "blur(100px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Тёплое дыхание при активности — очень сдержанно. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1600ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 115%, rgba(180,168,196,0.14), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Туман у нижней кромки + виньетка. */}
      <div className="absolute inset-x-0 bottom-0 h-[36%] bg-[linear-gradient(to_top,rgba(20,26,40,0.55),transparent)]" />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_40%,rgba(0,0,0,0.55))]" />
    </div>
  );
}
