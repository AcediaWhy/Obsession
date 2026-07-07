import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";

// Фон темы «Russia»: зимняя ночь, одинокий фонарь на пустой улице, метель.
// Кинематографичная версия — глубина резкости (крупный расфокус-снег спереди →
// мелкий вдали), параллакс за курсором, волюметрический конус с пылью, порывы
// ветра, зерно плёнки. Холодный стальной сумрак, тёплый натриевый свет; снег
// вспыхивает, пролетая сквозь луч. При активном обходе фонарь горит ровнее и
// теплее — «стало чуть спокойнее».
export function RussiaField() {
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

    // Мягкий диск-спрайт снежинки (расфокус без дорогого shadowBlur).
    const softSprite = (r: number, g: number, b: number): HTMLCanvasElement => {
      const s = 48;
      const c = document.createElement("canvas");
      c.width = s;
      c.height = s;
      const cc = c.getContext("2d")!;
      const gr = cc.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
      gr.addColorStop(0, `rgba(${r},${g},${b},0.95)`);
      gr.addColorStop(0.4, `rgba(${r},${g},${b},0.5)`);
      gr.addColorStop(1, `rgba(${r},${g},${b},0)`);
      cc.fillStyle = gr;
      cc.fillRect(0, 0, s, s);
      return c;
    };
    const coldSprite = softSprite(205, 216, 235);
    const warmSprite = softSprite(255, 224, 170);

    // Тайл зерна плёнки — рисуется раз, крутится сдвигом каждый кадр.
    const noiseTile = document.createElement("canvas");
    noiseTile.width = noiseTile.height = 128;
    {
      const nc = noiseTile.getContext("2d")!;
      const img = nc.createImageData(128, 128);
      for (let i = 0; i < img.data.length; i += 4) {
        const v = (Math.random() * 255) | 0;
        img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
        img.data[i + 3] = 255;
      }
      nc.putImageData(img, 0, 0);
    }
    const grainPattern = ctx.createPattern(noiseTile, "repeat");

    // Слои снега с разной глубиной: near — крупный/быстрый/сильный параллакс.
    type Flake = { x: number; y: number; r: number; vy: number; a: number };
    type Layer = { flakes: Flake[]; par: number; windK: number; rMin: number; rMax: number };
    const layers: Layer[] = [
      { flakes: [], par: 0.18, windK: 0.85, rMin: 0.8, rMax: 1.8 }, // far
      { flakes: [], par: 0.45, windK: 1.0, rMin: 1.6, rMax: 3.6 }, // mid
      { flakes: [], par: 1.0, windK: 1.25, rMin: 9, rMax: 20 }, // near (DOF)
    ];

    // Пылинки в конусе света.
    type Mote = { u: number; side: number; speed: number; sway: number; ph: number; sz: number };
    let motes: Mote[] = [];

    const seed = () => {
      const far = Math.max(100, Math.min(240, Math.round((w * h) / 9000)));
      const mid = Math.max(60, Math.min(160, Math.round((w * h) / 16000)));
      const near = 14;
      const counts = [far, mid, near];
      layers.forEach((L, i) => {
        L.flakes = Array.from({ length: counts[i] }, () => ({
          x: Math.random() * (w + h),
          y: Math.random() * h,
          r: L.rMin + Math.random() * (L.rMax - L.rMin),
          vy: (i === 2 ? 230 : i === 1 ? 140 : 95) + Math.random() * (i === 2 ? 120 : 90),
          a: i === 2 ? 0.1 + Math.random() * 0.12 : 0.22 + Math.random() * 0.4,
        }));
      });
      motes = Array.from({ length: 24 }, () => ({
        u: Math.random(),
        side: Math.random() * 2 - 1,
        speed: 0.03 + Math.random() * 0.05,
        sway: 0.2 + Math.random() * 0.5,
        ph: Math.random() * Math.PI * 2,
        sz: 1 + Math.random() * 2.5,
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

    // Параллакс за курсором (слушаем окно — Field pointer-events-none).
    const target = { x: 0, y: 0 };
    const parallax = { x: 0, y: 0 };
    const onMove = (e: PointerEvent) => {
      target.x = (e.clientX / window.innerWidth) * 2 - 1;
      target.y = (e.clientY / window.innerHeight) * 2 - 1;
    };
    window.addEventListener("pointermove", onMove);

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
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.0));
      parallax.x += (target.x - parallax.x) * 0.05;
      parallax.y += (target.y - parallax.y) * 0.05;

      ctx.clearRect(0, 0, w, h);

      // Порывы ветра — снег идёт волнами, а не константой.
      const gust = 0.55 + 0.4 * Math.sin(t * 0.3) + 0.2 * Math.sin(t * 0.11 + 1.3);

      // Геометрия фонаря (+ едва заметный собственный параллакс — якорь сцены).
      const lampX = w * 0.66 + parallax.x * w * 0.01;
      const lightY = h * 0.24;
      const groundY = h * 0.9;
      const flicker = 1 - (1 - warm) * (0.12 * (0.5 + 0.5 * Math.sin(t * 9 + Math.sin(t * 3))));
      const lampWarm = 0.7 + warm * 0.3;
      const halfAt = (y: number) => w * (0.02 + 0.14 * Math.max(0, (y - lightY) / (groundY - lightY)));

      ctx.globalCompositeOperation = "lighter";

      // Волюметрический конус: три вложенных полигона (гало → тело → ядро).
      const drawCone = (widthK: number, alpha: number) => {
        const topH = w * 0.02 * widthK;
        const botH = w * 0.16 * widthK;
        const cone = ctx.createLinearGradient(0, lightY, 0, groundY);
        cone.addColorStop(0, `rgba(230,190,120,${alpha * flicker * lampWarm})`);
        cone.addColorStop(0.5, `rgba(232,192,124,${alpha * 0.7 * flicker * lampWarm})`);
        cone.addColorStop(1, "rgba(210,180,130,0)");
        ctx.fillStyle = cone;
        ctx.beginPath();
        ctx.moveTo(lampX - topH, lightY);
        ctx.lineTo(lampX + topH, lightY);
        ctx.lineTo(lampX + botH, groundY);
        ctx.lineTo(lampX - botH, groundY);
        ctx.closePath();
        ctx.fill();
      };
      drawCone(1.5, 0.06);
      drawCone(1.0, 0.14);
      drawCone(0.5, 0.1);

      // Тёплое гало у лампы.
      const glowR = w * 0.12;
      const glow = ctx.createRadialGradient(lampX, lightY, 0, lampX, lightY, glowR);
      glow.addColorStop(0, `rgba(255,214,150,${0.5 * flicker * lampWarm})`);
      glow.addColorStop(0.4, `rgba(240,190,120,${0.18 * flicker})`);
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(lampX, lightY, glowR, 0, Math.PI * 2);
      ctx.fill();

      // Пылинки в конусе (ярче у лампы, медленный дрейф вниз).
      for (const m of motes) {
        m.u += m.speed * dt;
        if (m.u > 1) m.u -= 1;
        const y = lightY + m.u * (groundY - lightY);
        const x = lampX + m.side * halfAt(y) * 0.8 + Math.sin(t * m.sway + m.ph) * w * 0.01;
        const a = (1 - m.u) * 0.5 * flicker * lampWarm;
        if (a <= 0.01) continue;
        ctx.fillStyle = `rgba(255,226,175,${a})`;
        ctx.beginPath();
        ctx.arc(x, y, m.sz, 0, Math.PI * 2);
        ctx.fill();
      }

      // Снег: слои от дальнего к ближнему (ближний сверху — сильный расфокус).
      layers.forEach((L, li) => {
        const shift = parallax.x * L.par * w * 0.03;
        const shiftY = parallax.y * L.par * h * 0.015;
        for (const f of L.flakes) {
          f.y += f.vy * dt;
          f.x -= f.vy * dt * L.windK * gust * 0.9;
          if (f.y - f.r > h || f.x < -L.rMax * 2) {
            f.y = -Math.random() * h * 0.2;
            f.x = Math.random() * (w + h);
          }
          const dx = f.x + shift;
          const dy = f.y + shiftY;
          const inCone = dy > lightY && Math.abs(dx - lampX) < halfAt(dy);
          const sprite = inCone ? warmSprite : coldSprite;
          const boost = inCone ? 1.5 : li === 2 ? 1 : 0.8 + warm * 0.1;
          ctx.globalAlpha = Math.min(1, f.a * boost);
          const d = f.r * 2;
          ctx.drawImage(sprite, dx - f.r, dy - f.r, d, d);
        }
      });
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";

      // Силуэт фонаря поверх света: столб + голова-«кобра» + тёплая точка лампы.
      ctx.strokeStyle = "rgba(6,8,12,0.92)";
      ctx.lineWidth = Math.max(2, w * 0.006);
      ctx.beginPath();
      ctx.moveTo(lampX, lightY + h * 0.02);
      ctx.lineTo(lampX, groundY);
      ctx.stroke();
      ctx.fillStyle = "rgba(6,8,12,0.92)";
      ctx.beginPath();
      ctx.ellipse(lampX, lightY, w * 0.02, h * 0.012, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = "lighter";
      ctx.fillStyle = `rgba(255,220,160,${0.9 * flicker})`;
      ctx.beginPath();
      ctx.arc(lampX, lightY + h * 0.004, Math.max(1.5, w * 0.004), 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = "source-over";

      // Зерно плёнки — крутим тайл сдвигом, мягкий overlay.
      if (grainPattern) {
        ctx.save();
        ctx.globalCompositeOperation = "overlay";
        ctx.globalAlpha = 0.05;
        ctx.translate((Math.random() * 128) | 0, (Math.random() * 128) | 0);
        ctx.fillStyle = grainPattern;
        ctx.fillRect(-128, -128, w + 256, h + 256);
        ctx.restore();
      }
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      window.removeEventListener("pointermove", onMove);
    };
  }, []);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Ночное небо — холодный стальной сумрак. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_66%_22%,#12151f_0%,#0a0c13_46%,#050609_100%)]" />
      {/* Снежная земля внизу. */}
      <div className="absolute inset-x-0 bottom-0 h-[16%] bg-[linear-gradient(to_top,rgba(40,48,66,0.8),transparent)]" />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Виньетка — усиливает одиночество сцены. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_34%,rgba(0,0,0,0.66))]" />
    </div>
  );
}
