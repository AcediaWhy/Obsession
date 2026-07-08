import { useEffect, useRef } from "react";
import { renderActive } from "../render";

// Генеративный снег-оверлей поверх фото (2D canvas) + мягкое зерно плёнки.
// «Оживляет» статичную фотографию плотным падающим снегом — гибридная часть
// темы Russia. Детализация: несколько слоёв глубины (от мельчайшей «крупы»
// вдали до крупных мягких бокэ-хлопьев вблизи), штрихи-следы у быстрых хлопьев
// (тяжёлый снегопад, как на референсах), мерцание, порывы ветра. Холодная сине-
// белая палитра. Уважает `renderActive` (пауза кадров вне фокуса).

type Flake = {
  x: number;
  y: number;
  r: number;
  vy: number;
  a: number;
  ph: number; // фаза мерцания/покачивания
  sw: number; // амплитуда бокового покачивания
};

type LayerCfg = {
  // Плотность: 1 хлопье на `density` px² площади (меньше → плотнее). cap — потолок.
  density: number;
  cap: number;
  fixed?: number; // фиксированное число (для ближнего слоя)
  rMin: number;
  rMax: number;
  vMin: number;
  vMax: number;
  aMin: number;
  aMax: number;
  wind: number;
  soft: boolean; // мягкий спрайт (бокэ) vs чёткая крупица
  streak: boolean; // рисовать след-штрих
};

// Слои от дальнего к ближнему. Плотный мелкий снег вдали + редкий крупный вблизи.
const LAYERS: LayerCfg[] = [
  { density: 3200, cap: 620, rMin: 0.4, rMax: 0.95, vMin: 32, vMax: 70, aMin: 0.1, aMax: 0.26, wind: 0.7, soft: false, streak: false },
  { density: 7000, cap: 360, rMin: 0.9, rMax: 1.8, vMin: 55, vMax: 115, aMin: 0.22, aMax: 0.5, wind: 0.9, soft: false, streak: false },
  { density: 15000, cap: 190, rMin: 1.7, rMax: 3.6, vMin: 100, vMax: 185, aMin: 0.3, aMax: 0.62, wind: 1.05, soft: false, streak: true },
  { density: 0, cap: 0, fixed: 18, rMin: 7, rMax: 17, vMin: 210, vMax: 340, aMin: 0.07, aMax: 0.17, wind: 1.3, soft: true, streak: true },
];

export function SnowLayer() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const Q = 0.85; // чуть выше прежнего — мелкая крупа читается чётче
    let w = 0;
    let h = 0;

    // Мягкий диск-спрайт (бокэ для ближних хлопьев).
    const softSprite = (() => {
      const s = 48;
      const c = document.createElement("canvas");
      c.width = c.height = s;
      const cc = c.getContext("2d")!;
      const gr = cc.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
      gr.addColorStop(0, "rgba(228,236,248,0.95)");
      gr.addColorStop(0.4, "rgba(214,224,240,0.45)");
      gr.addColorStop(1, "rgba(214,224,240,0)");
      cc.fillStyle = gr;
      cc.fillRect(0, 0, s, s);
      return c;
    })();

    // Зерно плёнки — тайл рисуется раз, крутится сдвигом.
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
    const grain = ctx.createPattern(noiseTile, "repeat");

    const layers: Flake[][] = LAYERS.map(() => []);

    const seed = () => {
      LAYERS.forEach((cfg, i) => {
        const n = cfg.fixed ?? Math.max(30, Math.min(cfg.cap, Math.round((w * h) / cfg.density)));
        layers[i] = Array.from({ length: n }, () => ({
          x: Math.random() * (w + h),
          y: Math.random() * h,
          r: cfg.rMin + Math.random() * (cfg.rMax - cfg.rMin),
          vy: cfg.vMin + Math.random() * (cfg.vMax - cfg.vMin),
          a: cfg.aMin + Math.random() * (cfg.aMax - cfg.aMin),
          ph: Math.random() * Math.PI * 2,
          sw: 0.4 + Math.random() * 1.2,
        }));
      });
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

      ctx.clearRect(0, 0, w, h);

      // Порывы ветра — снег идёт волнами, а не константой.
      const gust = 0.55 + 0.4 * Math.sin(t * 0.26) + 0.2 * Math.sin(t * 0.09 + 1.1);

      LAYERS.forEach((cfg, li) => {
        const flakes = layers[li];
        const windBase = cfg.wind * gust;
        for (const f of flakes) {
          f.y += f.vy * dt;
          const dx = f.vy * dt * windBase * 0.7;
          f.x -= dx;
          f.x += Math.sin(t * 0.7 + f.ph) * f.sw * dt * 8; // покачивание
          if (f.y - f.r > h || f.x < -cfg.rMax * 2) {
            f.y = -Math.random() * h * 0.15;
            f.x = Math.random() * (w + h);
          }

          // Мерцание — лёгкая пульсация прозрачности.
          const tw = 0.78 + 0.22 * Math.sin(t * 2.2 + f.ph);
          const alpha = f.a * tw;

          // След-штрих у быстрых хлопьев — ощущение тяжёлого снегопада.
          if (cfg.streak) {
            const len = f.vy * 0.018;
            ctx.strokeStyle = `rgba(220,230,246,${alpha * 0.35})`;
            ctx.lineWidth = Math.max(0.6, f.r * 0.5);
            ctx.beginPath();
            ctx.moveTo(f.x, f.y);
            ctx.lineTo(f.x + windBase * len * 0.7, f.y - len);
            ctx.stroke();
          }

          if (cfg.soft) {
            ctx.globalAlpha = alpha;
            const d = f.r * 2;
            ctx.drawImage(softSprite, f.x - f.r, f.y - f.r, d, d);
            ctx.globalAlpha = 1;
          } else {
            // Чёткая крупица — читается как детальный мелкий снег.
            ctx.fillStyle = `rgba(226,234,248,${alpha})`;
            ctx.beginPath();
            ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2);
            ctx.fill();
          }
        }
      });

      // Зерно плёнки — мягкий overlay поверх снега.
      if (grain) {
        ctx.save();
        ctx.globalCompositeOperation = "overlay";
        ctx.globalAlpha = 0.05;
        ctx.translate((Math.random() * 128) | 0, (Math.random() * 128) | 0);
        ctx.fillStyle = grain;
        ctx.fillRect(-128, -128, w + 256, h + 256);
        ctx.restore();
      }
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return <canvas ref={ref} className="absolute inset-0 h-full w-full" />;
}
