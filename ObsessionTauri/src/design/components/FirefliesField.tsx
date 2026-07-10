import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, FPS_FIELD } from "../render";
import { createSpriteCache } from "./glowSprite";

// Фон темы «Fireflies»: тёплый летний сумрак. Глубокое индиго-небо к верху,
// тёмная тёпло-зелёная земля к низу, по которой лениво дрейфуют и тихо
// перемигиваются светлячки. Внизу — силуэты качающейся травы. Тихо, живо,
// спокойно. При активном обходе/прокси рой разгорается теплее и ярче — «дом
// рядом». Чистый canvas 2D, внутренний down-scale Q под мягкие свечения.
// Свечение светлячка, запечённое в спрайт по корзинам warm (цвет зависит только
// от warm). Альфы стопов линейны по яркости → яркость уходит в globalAlpha,
// радиус — в размер drawImage. Раньше: до 70 createRadialGradient КАЖДЫЙ кадр.
const flySprite = createSpriteCache(9, 64, (sctx, px, k) => {
  const r = px / 2;
  const cr = Math.round(196 + k * 40);
  const cg = Math.round(224 - k * 10);
  const cb = Math.round(130 - k * 40);
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, `rgba(${cr},${cg},${cb},1)`);
  g.addColorStop(0.35, `rgba(${cr},${cg - 20},${cb},0.5)`);
  g.addColorStop(1, `rgba(${cr},${cg},${cb},0)`);
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

export function FirefliesField() {
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

    // Мягкие свечения прощают низкое внутреннее разрешение — рисуем в Q от CSS-px
    // и растягиваем канвас на весь фон (главный тормоз WebView2 — анимация под
    // backdrop-filter, поэтому пикселей меньше = легче).
    const Q = 0.6;
    let w = 0;
    let h = 0;

    // Светлячок: позиция, курс (медленно поворачивает), скорость, фазы мигания.
    type Fly = {
      x: number;
      y: number;
      heading: number; // направление дрейфа, радианы
      turn: number; // скорость поворота курса
      spd: number; // px/сек
      size: number; // радиус свечения, px
      base: number; // базовая яркость 0..1
      blinkSpd: number;
      blinkPh: number;
      flSpd: number; // редкая яркая вспышка
      flPh: number;
    };
    let flies: Fly[] = [];

    // Травинка: якорь у низа, высота, толщина, фаза и амплитуда качания.
    type Blade = { x: number; hgt: number; wid: number; swPh: number; swSpd: number; sway: number };
    let grass: Blade[] = [];

    const seed = () => {
      const count = Math.round((w * h) / 26000);
      const n = Math.max(26, Math.min(70, count));
      flies = Array.from({ length: n }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        heading: Math.random() * Math.PI * 2,
        turn: (Math.random() - 0.5) * 0.5,
        spd: 6 + Math.random() * 12,
        size: 5 + Math.random() * 9,
        base: 0.3 + Math.random() * 0.45,
        blinkSpd: 0.5 + Math.random() * 1.3,
        blinkPh: Math.random() * Math.PI * 2,
        flSpd: 0.1 + Math.random() * 0.3,
        flPh: Math.random() * Math.PI * 2,
      }));

      const gn = Math.max(18, Math.min(60, Math.round(w / 26)));
      grass = Array.from({ length: gn }, () => ({
        x: Math.random() * w,
        hgt: h * (0.08 + Math.random() * 0.16),
        wid: 3 + Math.random() * 6,
        swPh: Math.random() * Math.PI * 2,
        swSpd: 0.4 + Math.random() * 0.6,
        sway: 6 + Math.random() * 12,
      }));
    };

    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * Q));
      canvas.height = Math.max(1, Math.round(h * Q));
      ctx.setTransform(Q, 0, 0, Q, 0, 0); // рисуем в CSS-px, движок ужимает в Q
      seed();
    };
    resize();
    window.addEventListener("resize", resize);

    // Тёплое свечение светлячка — готовый спрайт (см. flySprite выше): яркость в
    // globalAlpha, радиус в размер drawImage, цвет по корзине warm.
    const drawFly = (x: number, y: number, r: number, a: number, warm: number) => {
      ctx.globalAlpha = a;
      ctx.drawImage(flySprite(warm), x - r, y - r, r * 2, r * 2);
    };

    let t = 0;
    let warm = 0;

    const draw = (dt: number) => {
      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, w, h);

      // Светлячки — аддитивно.
      ctx.globalCompositeOperation = "lighter";
      for (const f of flies) {
        // Плавный дрейф: курс лениво виляет, скорость слегка «дышит».
        f.heading += f.turn * dt + Math.sin(t * 0.3 + f.blinkPh) * 0.02;
        const sp = f.spd * (0.6 + 0.4 * Math.sin(t * 0.5 + f.flPh)) * (1 + warm * 0.25);
        f.x += Math.cos(f.heading) * sp * dt;
        f.y += Math.sin(f.heading) * sp * dt * 0.7; // по вертикали спокойнее
        // Мягкий возврат за край (тор).
        if (f.x < -20) f.x = w + 20;
        else if (f.x > w + 20) f.x = -20;
        if (f.y < -20) f.y = h + 20;
        else if (f.y > h + 20) f.y = -20;

        const blink = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * f.blinkSpd + f.blinkPh));
        const flare = Math.pow(Math.max(0, Math.sin(t * f.flSpd + f.flPh)), 10);
        const a = Math.min(1, f.base * blink * (1 + warm * 0.5) + flare * (0.4 + warm * 0.3));
        drawFly(f.x, f.y, f.size * (1 + flare * 0.6), a, warm);
      }
      ctx.globalAlpha = 1;

      // Трава — тёмный силуэт поверх, обычным режимом.
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = "rgba(6,12,10,0.92)";
      for (const b of grass) {
        const tipX = b.x + Math.sin(t * b.swSpd + b.swPh) * b.sway;
        const baseY = h + 2;
        const tipY = h - b.hgt;
        ctx.beginPath();
        ctx.moveTo(b.x - b.wid / 2, baseY);
        ctx.quadraticCurveTo(b.x - b.wid * 0.15, (baseY + tipY) / 2, tipX, tipY);
        ctx.quadraticCurveTo(b.x + b.wid * 0.15, (baseY + tipY) / 2, b.x + b.wid / 2, baseY);
        ctx.closePath();
        ctx.fill();
      }
    };
    // Кап 60 fps: полноэкранный фон под backdrop-filter-панелями (см. AuroraField).
    const loop = createRenderLoop(draw, { fps: FPS_FIELD });
    loop.start();

    return () => {
      loop.dispose();
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Сумеречное небо: индиго вверху → тёмная тёпло-зелёная земля внизу. */}
      <div className="absolute inset-0 bg-[linear-gradient(180deg,#0d1226_0%,#121a2b_38%,#0f1a1c_74%,#0a1210_100%)]" />
      {/* Низкое тёплое зарево горизонта. */}
      <div
        className="absolute left-1/2 bottom-[-6%] h-[420px] w-[720px] -translate-x-1/2 rounded-full opacity-40"
        style={{ background: "radial-gradient(ellipse, rgba(180,168,96,0.35), transparent 70%)", filter: "blur(90px)" }}
      />
      {/* Холодные пятна глубины неба. */}
      <div
        className="absolute left-[14%] top-[16%] h-[380px] w-[380px] rounded-full opacity-20"
        style={{ background: "radial-gradient(circle, rgba(78,96,150,0.5), transparent 68%)", filter: "blur(120px)" }}
      />
      <div
        className="absolute right-[12%] top-[26%] h-[340px] w-[340px] rounded-full opacity-15"
        style={{ background: "radial-gradient(circle, rgba(86,132,138,0.5), transparent 68%)", filter: "blur(120px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Тёплое «дыхание» при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1600ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 62%, rgba(214,196,110,0.10), transparent 58%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Мягкая виньетка. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_48%,rgba(0,0,0,0.55))]" />
    </div>
  );
}
