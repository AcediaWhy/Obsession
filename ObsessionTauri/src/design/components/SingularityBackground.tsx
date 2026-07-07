import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";

// Космический фон: звёздное поле с дрейфом + гравитационные кольца.
// Реагирует на состояние: при активном обходе/прокси пространство «разогревается».
export function SingularityBackground() {
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

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    let w = 0;
    let h = 0;
    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    // Звёзды с параллаксным дрейфом.
    type Star = { x: number; y: number; z: number; tw: number };
    const stars: Star[] = Array.from({ length: 150 }, () => ({
      x: Math.random(),
      y: Math.random(),
      z: 0.3 + Math.random() * 0.7,
      tw: Math.random() * Math.PI * 2,
    }));

    let t = 0;
    let warm = 0; // плавный переход к «разогретому» состоянию 0..1
    let raf = 0;
    const draw = () => {
      t += 0.005;
      // Плавно догоняем целевое состояние (тепло при активности).
      const target = hotRef.current ? 1 : 0;
      warm += (target - warm) * 0.03;
      ctx.clearRect(0, 0, w, h);

      // Гравитационные кольца — центр рядом с ядром (~34% ширины).
      const gx = w * 0.34;
      const gy = h * 0.44;
      ctx.save();
      ctx.translate(gx, gy);
      for (let i = 1; i <= 6; i++) {
        const r = i * 46 + Math.sin(t + i) * 4;
        ctx.beginPath();
        ctx.ellipse(0, 0, r, r * 0.42, 0, 0, Math.PI * 2);
        // При разогреве кольца теплеют (индиго → magenta).
        const rr = Math.round(99 + warm * 140);
        const gg = Math.round(102 - warm * 40);
        const bb = Math.round(241 - warm * 40);
        ctx.strokeStyle = `rgba(${rr},${gg},${bb},${0.05 - i * 0.005 + warm * 0.02})`;
        ctx.lineWidth = 1;
        ctx.stroke();
      }
      ctx.restore();

      // Звёзды (при разогреве дрейф чуть быстрее).
      const drift = 1 + warm * 0.8;
      for (const s of stars) {
        s.tw += 0.02;
        const px = s.x * w;
        const py = ((s.y + t * s.z * 0.02 * drift) % 1) * h;
        const flick = 0.5 + Math.sin(s.tw) * 0.5;
        const alpha = 0.12 + s.z * 0.4 * flick;
        ctx.beginPath();
        ctx.arc(px, py, s.z * 1.1, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(210,220,255,${alpha})`;
        ctx.fill();
      }
      raf = requestAnimationFrame(draw);
    };
    draw();

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Базовый градиент дальнего космоса. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_34%_44%,#0d1024_0%,#070812_45%,#040509_100%)]" />
      {/* Нэбулы — мягкие цветные пятна. */}
      <div
        className="absolute -left-[10%] top-[20%] h-[420px] w-[420px] rounded-full opacity-40"
        style={{ background: "radial-gradient(circle, rgba(99,102,241,0.5), transparent 65%)", filter: "blur(80px)" }}
      />
      <div
        className="absolute right-[6%] top-[8%] h-[360px] w-[360px] rounded-full opacity-30"
        style={{ background: "radial-gradient(circle, rgba(34,211,238,0.45), transparent 65%)", filter: "blur(80px)" }}
      />
      <div
        className="absolute bottom-[-8%] left-[40%] h-[380px] w-[380px] rounded-full opacity-25"
        style={{ background: "radial-gradient(circle, rgba(139,92,246,0.5), transparent 65%)", filter: "blur(90px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Тёплая небула у ядра — проявляется, когда обход активен. */}
      <div
        className="absolute left-[18%] top-[28%] h-[420px] w-[420px] rounded-full transition-opacity duration-[1200ms]"
        style={{
          background: "radial-gradient(circle, rgba(240,171,252,0.4), transparent 62%)",
          filter: "blur(90px)",
          opacity: hot ? 0.5 : 0,
        }}
      />
      {/* Виньетка для глубины. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_35%,rgba(0,0,0,0.5))]" />
    </div>
  );
}
