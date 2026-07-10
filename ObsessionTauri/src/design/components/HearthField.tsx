import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, FPS_FIELD } from "../render";
import { createSpriteCache } from "./glowSprite";

// Фон темы «Hearth»: тёплая тёмная комната у очага. Внизу тлеет ложе углей,
// от него лениво поднимаются искры и в тёплом свете плавают пылинки; по «стенам»
// ходит мягкий мерцающий отсвет пламени. Уютно, тихо, тепло. При активном
// обходе/прокси огонь разгорается ярче. Чистый canvas 2D, внутренний down-scale Q.
// Свечения частиц, запечённые в спрайты: яркость уходит в globalAlpha, радиус —
// в размер drawImage. Раньше этот фон создавал до ~129 createRadialGradient
// КАЖДЫЙ кадр (пылинки + искры + купол) — главный пожиратель main-thread.

// Пылинка: цвет константный — один статичный спрайт.
const moteSprite = createSpriteCache(1, 32, (sctx, px) => {
  const r = px / 2;
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, "rgba(250,214,150,1)");
  g.addColorStop(1, "rgba(250,214,150,0)");
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

// Искра: цвет зависит от пер-частичного rise (0 у низа → 1 у верха) — 8 корзин.
const sparkSprite = createSpriteCache(8, 32, (sctx, px, k) => {
  const r = px / 2;
  const cg = Math.round(150 + k * 70);
  const cb = Math.round(60 + k * 40);
  const g = sctx.createRadialGradient(r, r, 0, r, r, r);
  g.addColorStop(0, `rgba(255,${cg},${cb},1)`);
  g.addColorStop(0.4, `rgba(255,${Math.round(cg * 0.7)},${Math.round(cb * 0.5)},0.5)`);
  g.addColorStop(1, `rgba(255,${cg},${cb},0)`);
  sctx.fillStyle = g;
  sctx.fillRect(0, 0, px, px);
});

export function HearthField() {
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

    // Искра: поднимается от ложа углей, виляет вбок, мерцает, гаснет вверху.
    type Spark = { x: number; y: number; vy: number; wob: number; wobPh: number; size: number; heat: number };
    // Пылинка: медленно дрейфует в тёплом свете, чуть мерцает.
    type Mote = { x: number; y: number; drift: number; vy: number; size: number; base: number; twSpd: number; twPh: number };
    let sparks: Spark[] = [];
    let motes: Mote[] = [];

    const spawnSpark = (s: Spark) => {
      s.x = w * (0.2 + Math.random() * 0.6); // искры летят из центральной зоны очага
      s.y = h + Math.random() * 20;
      s.vy = 26 + Math.random() * 46;
      s.wob = 6 + Math.random() * 16;
      s.wobPh = Math.random() * Math.PI * 2;
      s.size = 1.2 + Math.random() * 2.4;
      s.heat = 0.6 + Math.random() * 0.4;
    };

    const seed = () => {
      const sn = Math.max(30, Math.min(80, Math.round((w * h) / 26000)));
      sparks = Array.from({ length: sn }, () => {
        const s: Spark = { x: 0, y: 0, vy: 0, wob: 0, wobPh: 0, size: 0, heat: 0 };
        spawnSpark(s);
        s.y = Math.random() * h; // на старте раскиданы по высоте, без «залпа»
        return s;
      });
      const mn = Math.max(16, Math.min(48, Math.round((w * h) / 52000)));
      motes = Array.from({ length: mn }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        drift: 4 + Math.random() * 10,
        vy: 2 + Math.random() * 6,
        size: 1 + Math.random() * 2,
        base: 0.14 + Math.random() * 0.22,
        twSpd: 0.4 + Math.random() * 1.0,
        twPh: Math.random() * Math.PI * 2,
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

    let t = 0;
    let warm = 0;

    const draw = (dt: number) => {
      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      // Отсвет пламени: тёплый купол у низа, яркость «пляшет» суммой синусов.
      const flick =
        0.62 +
        0.18 * Math.sin(t * 5.3) +
        0.12 * Math.sin(t * 11.7 + 1.3) +
        0.08 * Math.sin(t * 19.1 + 2.7);
      const glowA = (0.16 + warm * 0.22) * Math.max(0.35, flick);
      const gy = h * 1.02;
      const gr = h * (0.9 + warm * 0.15);
      const fire = ctx.createRadialGradient(w / 2, gy, 0, w / 2, gy, gr);
      fire.addColorStop(0, `rgba(255,176,88,${glowA})`);
      fire.addColorStop(0.4, `rgba(226,120,52,${glowA * 0.6})`);
      fire.addColorStop(1, "rgba(120,40,20,0)");
      ctx.fillStyle = fire;
      ctx.fillRect(0, 0, w, h);

      // Пылинки — тусклый ленивый дрейф в тёплом свете.
      for (const m of motes) {
        m.y -= m.vy * dt * 0.5;
        m.x += Math.sin(t * 0.4 + m.twPh) * m.drift * dt;
        if (m.y < -6) {
          m.y = h + 6;
          m.x = Math.random() * w;
        }
        const tw = 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * m.twSpd + m.twPh));
        const a = m.base * tw * (0.8 + warm * 0.4);
        const R = m.size * 3;
        ctx.globalAlpha = a;
        ctx.drawImage(moteSprite(0), m.x - R, m.y - R, R * 2, R * 2);
      }

      // Искры — яркие, поднимаются и гаснут к верху.
      for (const s of sparks) {
        s.y -= s.vy * dt * (1 + warm * 0.35);
        s.x += Math.sin(t * 1.4 + s.wobPh) * s.wob * dt;
        if (s.y < -8) spawnSpark(s);
        // Гаснет по мере подъёма (1 у низа → 0 у верха).
        const rise = Math.max(0, Math.min(1, s.y / h));
        const flkr = 0.7 + 0.3 * Math.sin(t * 8 + s.wobPh);
        const a = Math.min(1, s.heat * rise * flkr * (0.7 + warm * 0.5));
        const r = s.size * (0.8 + warm * 0.3);
        // Тёплый жар (у низа желтее, выше — оранжево-красный) — спрайт по rise.
        const R = r * 3;
        ctx.globalAlpha = a;
        ctx.drawImage(sparkSprite(rise), s.x - R, s.y - R, R * 2, R * 2);
      }
      ctx.globalAlpha = 1;

      ctx.globalCompositeOperation = "source-over";
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
      {/* Тёмная тёплая комната: угольный верх → тёплое ложе углей у низа. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_50%_108%,#3a1c0e_0%,#1c1109_42%,#120b07_72%,#0b0705_100%)]" />
      {/* Тёплые размытые пятна отсвета снизу. */}
      <div
        className="absolute left-1/2 bottom-[-10%] h-[520px] w-[820px] -translate-x-1/2 rounded-full opacity-45"
        style={{ background: "radial-gradient(ellipse, rgba(230,132,58,0.5), transparent 70%)", filter: "blur(100px)" }}
      />
      <div
        className="absolute left-[20%] bottom-[6%] h-[300px] w-[300px] rounded-full opacity-25"
        style={{ background: "radial-gradient(circle, rgba(240,170,90,0.45), transparent 68%)", filter: "blur(110px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Разгорание при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1600ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 95%, rgba(255,150,70,0.14), transparent 60%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Тёплая виньетка. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_44%,rgba(0,0,0,0.62))]" />
    </div>
  );
}
