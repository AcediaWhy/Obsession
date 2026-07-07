import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";

// Реактивная среда темы «Ophanim»: нисходящие столпы света (глориоль) + парящие
// пылинки в лучах. В покое — тускло-золотое; при активном обходе/прокси лучи
// разгораются, теплеют в магенту, по низу проступает свечение престола.
export function OphanimField() {
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

    // Пониженное разрешение — как в AuroraField: главный рычаг против лагов.
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

    // Наклонные столпы света, нисходящие сверху.
    type Shaft = {
      x: number; // доля ширины (верхняя точка)
      width: number; // доля ширины
      lean: number; // горизонтальный снос к низу, доля ширины
      speed: number; // скорость мерцания
      phase: number;
      cold: [number, number, number];
      hot: [number, number, number];
    };
    const shafts: Shaft[] = [
      { x: 0.22, width: 0.10, lean: 0.06, speed: 0.5, phase: 0.0, cold: [234, 179, 8], hot: [232, 121, 249] },
      { x: 0.40, width: 0.14, lean: -0.05, speed: 0.36, phase: 1.4, cold: [253, 224, 71], hot: [240, 171, 252] },
      { x: 0.60, width: 0.11, lean: 0.07, speed: 0.6, phase: 2.7, cold: [250, 204, 21], hot: [217, 70, 239] },
      { x: 0.80, width: 0.09, lean: -0.06, speed: 0.44, phase: 4.1, cold: [253, 230, 138], hot: [244, 114, 182] },
    ];

    // Пылинки, парящие в лучах.
    const N = 26;
    const motes = Array.from({ length: N }, (_, i) => ({
      x: ((i * 97) % 100) / 100,
      y: ((i * 53) % 100) / 100,
      r: 0.6 + ((i * 31) % 10) / 10,
      drift: 0.004 + ((i * 17) % 10) / 1000,
      sway: ((i * 13) % 100) / 100,
    }));

    let t = 0;
    let warm = 0;
    let raf = 0;
    let last = 0;
    const FRAME = 1000 / 30;

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0; // сброс, чтобы после паузы dt не «прыгнул»
        return;
      }
      if (now - last < FRAME) return;
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 0.033;
      last = now;

      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.4));
      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      // Столпы света: трапеции сверху вниз с вертикальным градиентом.
      for (const sh of shafts) {
        const topX = sh.x * w;
        const botX = topX + sh.lean * w;
        const half = (sh.width * w) / 2;
        const flick = 0.75 + Math.sin(t * sh.speed + sh.phase) * 0.25;

        const [cr, cg, cb] = sh.cold;
        const [hr, hg, hb] = sh.hot;
        const r = Math.round(cr + (hr - cr) * warm);
        const g = Math.round(cg + (hg - cg) * warm);
        const b = Math.round(cb + (hb - cb) * warm);
        const peak = (0.045 + warm * 0.07) * flick;

        ctx.beginPath();
        ctx.moveTo(topX - half * 0.5, 0);
        ctx.lineTo(topX + half * 0.5, 0);
        ctx.lineTo(botX + half, h);
        ctx.lineTo(botX - half, h);
        ctx.closePath();

        const grad = ctx.createLinearGradient(0, 0, 0, h);
        grad.addColorStop(0.0, `rgba(${r},${g},${b},${peak})`);
        grad.addColorStop(0.55, `rgba(${r},${g},${b},${peak * 0.6})`);
        grad.addColorStop(1.0, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = grad;
        ctx.fill();
      }

      // Пылинки — мягко плывут вверх, покачиваясь; ярче при разогреве.
      const mr = Math.round(253 - 13 * warm);
      const mg = Math.round(224 - 53 * warm);
      const mb = Math.round(71 + 181 * warm);
      for (const m of motes) {
        const y = (m.y - t * m.drift) % 1;
        const yy = (y < 0 ? y + 1 : y) * h;
        const xx = (m.x + Math.sin(t * 0.4 + m.sway * 6.28) * 0.01) * w;
        const a = (0.12 + warm * 0.28) * (0.4 + 0.6 * Math.sin(t * 1.5 + m.sway * 6.28) ** 2);
        ctx.beginPath();
        ctx.arc(xx, yy, m.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${mr},${mg},${mb},${a})`;
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
      {/* Базовый градиент глубины — тёплое тёмное золото. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_50%_0%,#161006_0%,#0a0806_46%,#050404_100%)]" />
      {/* Мягкие тёплые пятна для объёма. */}
      <div
        className="absolute left-[20%] -top-[6%] h-[420px] w-[420px] rounded-full opacity-35"
        style={{ background: "radial-gradient(circle, rgba(234,179,8,0.5), transparent 66%)", filter: "blur(90px)" }}
      />
      <div
        className="absolute right-[18%] -top-[4%] h-[360px] w-[360px] rounded-full opacity-25"
        style={{ background: "radial-gradient(circle, rgba(250,204,21,0.45), transparent 66%)", filter: "blur(90px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Свечение престола по низу — проступает при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1400ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 120%, rgba(240,171,252,0.16), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_38%,rgba(0,0,0,0.5))]" />
    </div>
  );
}
