import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";

// Запасной 2D-фон темы «Rain» (id japan) — показывается, только если WebGL
// недоступен или 3D-сцена упала. Косые струи дождя на холодном сланце, редкая
// рябь у нижней кромки. При активном обходе дождь усиливается («гроза»).
export function RainField2D() {
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

    type Streak = { x: number; y: number; len: number; speed: number; a: number };
    const ANGLE = 0.18;
    let streaks: Streak[] = [];
    const seed = () => {
      const count = Math.round((w * h) / 24000);
      streaks = Array.from({ length: Math.max(50, Math.min(180, count)) }, () => ({
        x: Math.random() * (w + h * ANGLE) - h * ANGLE,
        y: Math.random() * h,
        len: 12 + Math.random() * 24,
        speed: 300 + Math.random() * 380,
        a: 0.06 + Math.random() * 0.14,
      }));
    };

    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * Q));
      canvas.height = Math.max(1, Math.round(h * Q));
      ctx.setTransform(Q, 0, 0, Q, 0, 0);
      seed();
    };
    resize();
    window.addEventListener("resize", resize);

    type Ripple = { x: number; y: number; r: number; alpha: number };
    const ripples: Ripple[] = [];
    let rippleAcc = 0;

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
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, w, h);

      // «Гроза усиливается»: при активации дождь быстрее и плотнее.
      const speedK = 1 + warm * 0.5;
      const alphaK = 1 + warm * 0.6;

      for (const s of streaks) {
        s.y += s.speed * speedK * dt;
        s.x += s.speed * speedK * dt * ANGLE;
        if (s.y - s.len > h) {
          s.y = -Math.random() * h * 0.3;
          s.x = Math.random() * (w + h * ANGLE) - h * ANGLE;
        }
        ctx.strokeStyle = `rgba(150,175,210,${s.a * alphaK})`;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(s.x, s.y);
        ctx.lineTo(s.x - s.len * ANGLE, s.y - s.len);
        ctx.stroke();
      }

      rippleAcc += dt;
      const interval = 0.14 - warm * 0.06;
      while (rippleAcc > interval) {
        rippleAcc -= interval;
        ripples.push({ x: Math.random() * w, y: h - 6 - Math.random() * h * 0.05, r: 1, alpha: 0.16 + warm * 0.06 });
      }
      ctx.globalCompositeOperation = "lighter";
      for (let i = ripples.length - 1; i >= 0; i--) {
        const rp = ripples[i];
        rp.r += 48 * dt;
        rp.alpha -= dt * 0.22;
        if (rp.alpha <= 0 || rp.r > 48) {
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
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_50%_18%,#141a28_0%,#0b0f18_50%,#05070c_100%)]" />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      <div className="absolute inset-x-0 bottom-0 h-[34%] bg-[linear-gradient(to_top,rgba(20,26,40,0.55),transparent)]" />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_42%,rgba(0,0,0,0.5))]" />
    </div>
  );
}
