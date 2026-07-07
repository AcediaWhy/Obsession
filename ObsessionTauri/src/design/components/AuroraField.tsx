import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";

// Реактивная среда: полноэкранные шторы полярного сияния. В покое — прохладный
// индиго/циан-дрейф; при активном обходе/прокси пространство «разогревается» —
// шторы ускоряются, теплеют (magenta/hot) и по краям проступает свечение.
export function AuroraField() {
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

    // Намеренно рендерим фон в пониженном разрешении: блюр/виньетка скрывают
    // мягкость, зато fill-rate падает в разы (главный источник лагов в WebView2).
    const Q = 0.6;
    let w = 0;
    let h = 0;
    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * Q));
      canvas.height = Math.max(1, Math.round(h * Q));
      ctx.setTransform(Q, 0, 0, Q, 0, 0); // рисуем в координатах CSS-пикселей
    };
    resize();
    window.addEventListener("resize", resize);

    // Крупные вертикальные шторы, дрейфующие по горизонтали.
    type Curtain = {
      x: number; // доля ширины
      width: number; // доля ширины
      amp: number; // доля ширины
      freq: number;
      speed: number;
      phase: number;
      cold: [number, number, number];
      hot: [number, number, number];
    };
    const curtains: Curtain[] = [
      { x: 0.18, width: 0.16, amp: 0.05, freq: 1.1, speed: 0.10, phase: 0.0, cold: [99, 102, 241], hot: [139, 92, 246] },
      { x: 0.42, width: 0.22, amp: 0.06, freq: 0.8, speed: 0.07, phase: 1.6, cold: [34, 211, 238], hot: [240, 171, 252] },
      { x: 0.66, width: 0.18, amp: 0.055, freq: 1.3, speed: 0.12, phase: 3.0, cold: [129, 140, 248], hot: [56, 189, 248] },
      { x: 0.85, width: 0.14, amp: 0.045, freq: 1.0, speed: 0.09, phase: 4.5, cold: [56, 189, 248], hot: [253, 230, 138] },
    ];

    let t = 0;
    let warm = 0;
    let raf = 0;
    let last = 0;
    const FRAME = 1000 / 30; // ambient-фон: 30 fps более чем достаточно

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0; // сброс, чтобы после паузы dt не «прыгнул»
        return;
      }
      if (now - last < FRAME) return; // throttle
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0.033;
      last = now;

      // Время и разогрев — кадронезависимые (одинаковая скорость на любом мониторе).
      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.4));
      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      const step = 14;
      for (const c of curtains) {
        const cx = c.x * w;
        const half = (c.width * w) / 2;
        const amp = c.amp * w * (1 + warm * 0.5);
        const drift = c.speed * (1 + warm * 0.9);

        const centerAt = (y: number) => {
          const u = y / h;
          return (
            cx +
            Math.sin(u * Math.PI * 2 * c.freq + t * drift * 6 + c.phase) * amp +
            Math.sin(u * Math.PI * 1.3 * c.freq - t * drift * 3 + c.phase) * amp * 0.5
          );
        };

        ctx.beginPath();
        for (let y = -step; y <= h + step; y += step) {
          const x = centerAt(y) - half;
          y === -step ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        }
        for (let y = h + step; y >= -step; y -= step) {
          ctx.lineTo(centerAt(y) + half, y);
        }
        ctx.closePath();

        const [cr, cg, cb] = c.cold;
        const [hr, hg, hb] = c.hot;
        const r = Math.round(cr + (hr - cr) * warm);
        const g = Math.round(cg + (hg - cg) * warm);
        const b = Math.round(cb + (hb - cb) * warm);
        const peak = 0.05 + warm * 0.06;

        const grad = ctx.createLinearGradient(0, 0, 0, h);
        grad.addColorStop(0.0, `rgba(${r},${g},${b},0)`);
        grad.addColorStop(0.4, `rgba(${r},${g},${b},${peak})`);
        grad.addColorStop(1.0, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = grad;
        ctx.fill();
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
      {/* Базовый градиент глубины. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_30%_30%,#0c1024_0%,#070810_48%,#040509_100%)]" />
      {/* Мягкие цветные пятна для объёма. */}
      <div
        className="absolute -left-[8%] top-[12%] h-[440px] w-[440px] rounded-full opacity-40"
        style={{ background: "radial-gradient(circle, rgba(99,102,241,0.5), transparent 66%)", filter: "blur(90px)" }}
      />
      <div
        className="absolute right-[4%] top-[6%] h-[380px] w-[380px] rounded-full opacity-30"
        style={{ background: "radial-gradient(circle, rgba(34,211,238,0.45), transparent 66%)", filter: "blur(90px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Тёплое свечение по краю — проступает при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1400ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 120%, rgba(240,171,252,0.16), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Виньетка для глубины. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_38%,rgba(0,0,0,0.5))]" />
    </div>
  );
}
