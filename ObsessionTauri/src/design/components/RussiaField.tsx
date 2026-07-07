import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";

// Фон темы «Russia»: зимняя ночь, одинокий фонарь на пустой улице, метель.
// Холодный стальной сумрак, тёплый конус натриевого света; снег летит по ветру
// и вспыхивает, пролетая сквозь свет. Депрессивный, тихий вайб. При активном
// обходе фонарь горит ровнее и теплее — «стало чуть спокойнее».
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
    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * Q));
      canvas.height = Math.max(1, Math.round(h * Q));
      ctx.setTransform(Q, 0, 0, Q, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    // Снег метели: летит по ветру (сильный горизонтальный снос).
    type Flake = { x: number; y: number; r: number; vy: number; a: number };
    const WIND = 0.55;
    let flakes: Flake[] = [];
    const seed = () => {
      const n = Math.max(80, Math.min(260, Math.round((w * h) / 12000)));
      flakes = Array.from({ length: n }, () => ({
        x: Math.random() * (w + h),
        y: Math.random() * h,
        r: 0.8 + Math.random() * 2.2,
        vy: 120 + Math.random() * 200,
        a: 0.25 + Math.random() * 0.55,
      }));
    };
    seed();

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

      ctx.clearRect(0, 0, w, h);

      // Геометрия фонаря.
      const lampX = w * 0.66;
      const lightY = h * 0.24; // источник света
      const groundY = h * 0.9;
      // Лёгкое мерцание; при активации почти пропадает (ровный свет).
      const flicker = 1 - (1 - warm) * (0.12 * (0.5 + 0.5 * Math.sin(t * 9 + Math.sin(t * 3))));
      const lampWarm = 0.7 + warm * 0.3;

      // Конус света вниз к земле.
      ctx.globalCompositeOperation = "lighter";
      const cone = ctx.createLinearGradient(0, lightY, 0, groundY);
      cone.addColorStop(0, `rgba(${230},${190},${120},${0.16 * flicker * lampWarm})`);
      cone.addColorStop(1, "rgba(210,180,130,0)");
      ctx.fillStyle = cone;
      ctx.beginPath();
      ctx.moveTo(lampX - w * 0.02, lightY);
      ctx.lineTo(lampX + w * 0.02, lightY);
      ctx.lineTo(lampX + w * 0.16, groundY);
      ctx.lineTo(lampX - w * 0.16, groundY);
      ctx.closePath();
      ctx.fill();

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

      // Снег. Внутри конуса — ярче и теплее (свет ловит хлопья).
      for (const f of flakes) {
        f.y += f.vy * dt;
        f.x -= f.vy * dt * WIND; // ветер влево
        if (f.y > h || f.x < -10) {
          f.y = -Math.random() * h * 0.2;
          f.x = Math.random() * (w + h);
        }
        // Внутри конуса?
        const cw = (f.y - lightY) / (groundY - lightY);
        const halfW = w * (0.02 + 0.14 * Math.max(0, cw));
        const inCone = f.y > lightY && Math.abs(f.x - lampX) < halfW;
        if (inCone) {
          ctx.fillStyle = `rgba(255,224,170,${Math.min(f.a * 1.6, 1)})`;
        } else {
          ctx.fillStyle = `rgba(200,214,235,${f.a * (0.7 + warm * 0.1)})`;
        }
        ctx.beginPath();
        ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalCompositeOperation = "source-over";

      // Силуэт фонаря (тёмный) поверх света: столб + голова-«кобра».
      ctx.strokeStyle = "rgba(6,8,12,0.92)";
      ctx.lineWidth = Math.max(2, w * 0.006);
      ctx.beginPath();
      ctx.moveTo(lampX, lightY + h * 0.02);
      ctx.lineTo(lampX, groundY);
      ctx.stroke();
      // Голова фонаря.
      ctx.fillStyle = "rgba(6,8,12,0.92)";
      ctx.beginPath();
      ctx.ellipse(lampX, lightY, w * 0.02, h * 0.012, 0, 0, Math.PI * 2);
      ctx.fill();
      // Маленькая тёплая точка лампы.
      ctx.fillStyle = `rgba(255,220,160,${0.9 * flicker})`;
      ctx.beginPath();
      ctx.arc(lampX, lightY + h * 0.004, Math.max(1.5, w * 0.004), 0, Math.PI * 2);
      ctx.fill();
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
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
