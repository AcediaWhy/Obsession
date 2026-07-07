import { useEffect, useRef } from "react";
import { renderActive } from "../render";

// Оверлей сакуры поверх любой темы: густо падающие лепестки, покачиваются и
// вращаются на лету. Чисто декоративный, pointer-events-none.
export function SakuraOverlay() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    let w = 0;
    let h = 0;
    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = w;
      canvas.height = h;
    };
    resize();
    window.addEventListener("resize", resize);

    type Petal = {
      x: number;
      y: number;
      size: number;
      vy: number;
      sway: number;
      ph: number;
      rot: number;
      vrot: number;
      hue: number; // 0..1 от нежно-розового к насыщенному
    };
    let petals: Petal[] = [];
    const seed = () => {
      const n = Math.max(70, Math.min(180, Math.round((w * h) / 12000)));
      petals = Array.from({ length: n }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        size: 6 + Math.random() * 8,
        vy: 26 + Math.random() * 46,
        sway: 14 + Math.random() * 30,
        ph: Math.random() * Math.PI * 2,
        rot: Math.random() * Math.PI * 2,
        vrot: (Math.random() - 0.5) * 1.6,
        hue: Math.random(),
      }));
    };
    seed();
    let seededFor = w * h;

    const petalColor = (hue: number, a: number) => {
      // От нежно-розового (255,222,236) к насыщенному (247,168,205).
      const r = Math.round(255 - hue * 8);
      const g = Math.round(222 - hue * 54);
      const b = Math.round(236 - hue * 31);
      return `rgba(${r},${g},${b},${a})`;
    };

    const drawPetal = (p: Petal) => {
      ctx.save();
      ctx.translate(p.x, p.y);
      ctx.rotate(p.rot);
      // Лёгкое «складывание» лепестка через масштаб по X от угла покачивания.
      const fold = 0.5 + 0.5 * Math.abs(Math.cos(p.ph));
      ctx.scale(fold, 1);
      const s = p.size;
      ctx.beginPath();
      // Форма лепестка: две дуги с выемкой сверху.
      ctx.moveTo(0, -s * 0.5);
      ctx.bezierCurveTo(s * 0.5, -s * 0.5, s * 0.5, s * 0.4, 0, s * 0.6);
      ctx.bezierCurveTo(-s * 0.5, s * 0.4, -s * 0.5, -s * 0.5, 0, -s * 0.5);
      const grad = ctx.createLinearGradient(0, -s * 0.5, 0, s * 0.6);
      grad.addColorStop(0, petalColor(p.hue, 0.9));
      grad.addColorStop(1, petalColor(Math.min(1, p.hue + 0.3), 0.75));
      ctx.fillStyle = grad;
      ctx.fill();
      ctx.restore();
    };

    let t = 0;
    let raf = 0;
    let last = 0;

    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!renderActive()) {
        last = 0;
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.05) : 0.016;
      last = now;
      t += dt;

      if (Math.abs(w * h - seededFor) > seededFor * 0.4) {
        seed();
        seededFor = w * h;
      }

      ctx.clearRect(0, 0, w, h);
      for (const p of petals) {
        p.y += p.vy * dt;
        p.ph += dt * 1.4;
        p.x += Math.sin(t * 0.7 + p.ph) * p.sway * dt;
        p.rot += p.vrot * dt;
        if (p.y - p.size > h) {
          p.y = -p.size;
          p.x = Math.random() * w;
        }
        drawPetal(p);
      }
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute inset-0 z-40 h-full w-full"
    />
  );
}
