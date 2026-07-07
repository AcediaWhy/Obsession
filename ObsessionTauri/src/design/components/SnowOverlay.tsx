import { useEffect, useRef } from "react";
import { renderActive } from "../render";

// Оверлей снега поверх любой темы. Помимо падающих хлопьев — интерактивные
// сугробы: снег копится на элементах с классом `.snow-surface` (круглые
// помечаются data-snow="round"), до предела. Клик рядом с сугробом — снег
// осыпается вниз комками. Оверлей pointer-events-none; клики ловим на window,
// не мешая UI (preventDefault не вызываем).
export function SnowOverlay() {
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

    // Падающие хлопья (плотно — «побольше снега»).
    type Flake = { x: number; y: number; r: number; vy: number; sway: number; ph: number };
    let flakes: Flake[] = [];
    const seed = () => {
      const n = Math.max(150, Math.min(320, Math.round((w * h) / 6500)));
      flakes = Array.from({ length: n }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        r: 0.8 + Math.random() * 2.6,
        vy: 30 + Math.random() * 70,
        sway: 6 + Math.random() * 16,
        ph: Math.random() * Math.PI * 2,
      }));
    };
    seed();
    let seededFor = w * h;

    // Сугробы: аккумуляция по элементам. Ключ — сам DOM-узел.
    const acc = new Map<Element, number>();
    const MAX_RECT = 18;
    const MAX_ROUND = 15;
    const GROW = 4.2; // px/сек

    // Осыпающиеся комки снега.
    type Chunk = { x: number; y: number; vx: number; vy: number; r: number; life: number };
    const chunks: Chunk[] = [];

    const rectOf = (el: Element) => el.getBoundingClientRect();
    const isRound = (el: Element) => (el as HTMLElement).dataset.snow === "round";
    const maxOf = (el: Element) => (isRound(el) ? MAX_ROUND : MAX_RECT);

    // Клик рядом с сугробом — осыпаем.
    const onDown = (e: PointerEvent) => {
      const x = e.clientX;
      const y = e.clientY;
      acc.forEach((a, el) => {
        if (a < 4) return;
        const r = rectOf(el);
        let hit = false;
        if (isRound(el)) {
          const ecx = r.left + r.width / 2;
          const ecy = r.top + r.height / 2;
          const R = r.width / 2;
          const d = Math.hypot(x - ecx, y - ecy);
          hit = y < ecy && d > R - a - 10 && d < R + 10;
        } else {
          hit = x > r.left && x < r.right && y > r.top - a - 12 && y < r.top + 10;
        }
        if (hit) shed(el, a, r);
      });
    };
    window.addEventListener("pointerdown", onDown);

    const shed = (el: Element, a: number, r: DOMRect) => {
      const n = 12 + Math.round(a);
      for (let i = 0; i < n; i++) {
        let px: number;
        let py: number;
        if (isRound(el)) {
          const ang = Math.PI * 1.12 + Math.random() * Math.PI * 0.76;
          const R = r.width / 2;
          px = r.left + r.width / 2 + Math.cos(ang) * R;
          py = r.top + r.height / 2 + Math.sin(ang) * R;
        } else {
          px = r.left + Math.random() * r.width;
          py = r.top - Math.random() * a;
        }
        chunks.push({
          x: px,
          y: py,
          vx: (Math.random() - 0.5) * 60,
          vy: 30 + Math.random() * 70,
          r: 1.5 + Math.random() * 3,
          life: 1,
        });
      }
      acc.set(el, 0);
    };

    // Рисование шапки сугроба.
    const drawCap = (el: Element, a: number) => {
      if (a < 0.5) return;
      const r = rectOf(el);
      if (r.width < 4 || r.bottom < 0 || r.top > h) return;
      if (isRound(el)) {
        const ecx = r.left + r.width / 2;
        const ecy = r.top + r.height / 2;
        const R = r.width / 2;
        ctx.lineCap = "round";
        ctx.strokeStyle = "rgba(244,248,255,0.95)";
        ctx.lineWidth = a;
        ctx.beginPath();
        ctx.arc(ecx, ecy, R * 0.95, Math.PI * 1.12, Math.PI * 1.88);
        ctx.stroke();
        ctx.strokeStyle = "rgba(255,255,255,0.6)";
        ctx.lineWidth = Math.max(1, a * 0.4);
        ctx.beginPath();
        ctx.arc(ecx, ecy, R * 0.95, Math.PI * 1.16, Math.PI * 1.84);
        ctx.stroke();
      } else {
        const top = r.top;
        const grad = ctx.createLinearGradient(0, top - a, 0, top + 3);
        grad.addColorStop(0, "rgba(255,255,255,0.96)");
        grad.addColorStop(1, "rgba(226,234,247,0.9)");
        ctx.fillStyle = grad;
        ctx.beginPath();
        ctx.moveTo(r.left, top + 3);
        ctx.lineTo(r.left + 4, top - a * 0.5);
        // Волнистый верх сугроба.
        const seg = Math.max(3, Math.round(r.width / 28));
        for (let i = 0; i <= seg; i++) {
          const x = r.left + (r.width * i) / seg;
          const bump = Math.sin(i * 1.7 + r.left * 0.01) * a * 0.16;
          ctx.lineTo(x, top - a + bump);
        }
        ctx.lineTo(r.right - 4, top - a * 0.5);
        ctx.lineTo(r.right, top + 3);
        ctx.closePath();
        ctx.fill();
      }
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

      // Переинициализация плотности при заметном изменении площади.
      if (Math.abs(w * h - seededFor) > seededFor * 0.4) {
        seed();
        seededFor = w * h;
      }

      ctx.clearRect(0, 0, w, h);

      // Аккумуляция + шапки сугробов.
      const els = document.querySelectorAll(".snow-surface");
      const seen = new Set<Element>();
      els.forEach((el) => {
        seen.add(el);
        const a = Math.min(maxOf(el), (acc.get(el) ?? 0) + GROW * dt);
        acc.set(el, a);
        drawCap(el, a);
      });
      // Убираем размонтированные.
      acc.forEach((_, el) => {
        if (!seen.has(el)) acc.delete(el);
      });

      // Хлопья.
      ctx.fillStyle = "rgba(255,255,255,0.9)";
      for (const f of flakes) {
        f.y += f.vy * dt;
        f.x += Math.sin(t * 0.8 + f.ph) * f.sway * dt;
        if (f.y - f.r > h) {
          f.y = -f.r;
          f.x = Math.random() * w;
        }
        ctx.globalAlpha = 0.55 + (f.r / 3.4) * 0.4;
        ctx.beginPath();
        ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = 1;

      // Осыпающиеся комки.
      for (let i = chunks.length - 1; i >= 0; i--) {
        const c = chunks[i];
        c.vy += 900 * dt;
        c.x += c.vx * dt;
        c.y += c.vy * dt;
        c.life -= dt * 0.9;
        if (c.life <= 0 || c.y > h + 20) {
          chunks.splice(i, 1);
          continue;
        }
        ctx.globalAlpha = Math.min(1, c.life);
        ctx.fillStyle = "rgba(248,251,255,1)";
        ctx.beginPath();
        ctx.arc(c.x, c.y, c.r, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      window.removeEventListener("pointerdown", onDown);
    };
  }, []);

  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute inset-0 z-40 h-full w-full"
    />
  );
}
