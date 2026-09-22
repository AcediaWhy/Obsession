import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, frameQualityScale, useRenderActive, type QualityTier, type RenderLoop } from "../render";
import { FallenRuins } from "./FallenRuins";
import "../../styles/fallenRuins.css";

// Тихие подземные руины, золотые цветы и редкие огоньки. Иллюстрация статична;
// существующий общий цикл рисует только частицы и освобождается в трее.
export function FallenField({ paused = false }: { paused?: boolean }) {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;
  const motionActive = useRenderActive() && !paused;
  const hotRef = useRef(hot);
  hotRef.current = hot;

  const ref = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    let w = 0;
    let h = 0;
    let backingScale = frameQualityScale("high");

    // Звезда-сверкание: позиция, размер блока, фазы мерцания и редкой вспышки.
    type Star = {
      x: number;
      y: number;
      u: number; // размер пикс-блока
      drift: number; // скорость медленного оседания
      twSpeed: number;
      twPh: number;
      flSpeed: number;
      flPh: number;
      base: number; // базовая яркость
    };
    let stars: Star[] = [];

    const seed = () => {
      const count = Math.round((w * h) / 24000);
      const n = Math.max(40, Math.min(120, count));
      stars = Array.from({ length: n }, () => {
        const big = Math.random() < 0.3;
        return {
          x: Math.random() * w,
          y: Math.random() * h,
          u: big ? 1.5 + Math.random() * 1.2 : 0.8 + Math.random() * 0.7,
          drift: 2 + Math.random() * 6,
          twSpeed: 0.5 + Math.random() * 1.4,
          twPh: Math.random() * Math.PI * 2,
          flSpeed: 0.15 + Math.random() * 0.4,
          flPh: Math.random() * Math.PI * 2,
          base: 0.28 + Math.random() * 0.4,
        };
      });
    };

    const resize = (reseed = true) => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * backingScale));
      canvas.height = Math.max(1, Math.round(h * backingScale));
      ctx.setTransform(backingScale, 0, 0, backingScale, 0, 0);
      ctx.imageSmoothingEnabled = false;
      if (reseed) seed();
    };
    resize();
    // resize стирает кадр — под reduce-motion дорисуем стоп-кадр (живому — no-op).
    const onResize = () => {
      resize();
      loop.invalidate();
    };
    window.addEventListener("resize", onResize);

    // Пиксельное сверкание: центральный блок + четыре луча (крест).
    const drawStar = (x: number, y: number, u: number, a: number, arms: number, warm: number) => {
      const px = Math.floor(x);
      const py = Math.floor(y);
      const b = Math.max(1, Math.round(u));
      // Монохром: серебристо-белый, при активации едва теплее.
      const cr = 224;
      const cg = Math.round(228 - warm * 8);
      const cb = Math.round(238 - warm * 26);
      ctx.fillStyle = `rgba(${cr},${cg},${cb},${a})`;
      // центр
      ctx.fillRect(px - b, py - b, b * 2, b * 2);
      if (arms > 0.02) {
        const L = Math.round(b * (1.5 + arms * 3));
        const aw = Math.max(1, Math.round(b * 0.8));
        ctx.fillRect(px - aw, py - b - L, aw * 2, L); // вверх
        ctx.fillRect(px - aw, py + b, aw * 2, L); // вниз
        ctx.fillRect(px - b - L, py - aw, L, aw * 2); // влево
        ctx.fillRect(px + b, py - aw, L, aw * 2); // вправо
      }
    };

    let t = 0;
    // Сеем от текущего состояния щита — вход в тему без прогрева на глазах.
    let warm = hotRef.current ? 1 : 0;

    const draw = (dt: number) => {
      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.2));

      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      for (const s of stars) {
        // Медленное оседание вниз — «падение».
        s.y += s.drift * dt;
        if (s.y - 6 > h) {
          s.y = -6;
          s.x = Math.random() * w;
        }
        // Мерцание.
        const tw = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * s.twSpeed + s.twPh));
        // Редкая вспышка-«сохранение»: резкий пик, почти всегда 0.
        const flare = Math.pow(Math.max(0, Math.sin(t * s.flSpeed + s.flPh)), 10);
        const a = Math.min(1, s.base * tw * (1 + warm * 0.5) + flare * (0.5 + warm * 0.3));
        drawStar(s.x, s.y, s.u, a, flare, warm);
      }

      ctx.globalCompositeOperation = "source-over";
    };
    const onQualityChange = (qualityTier: QualityTier) => {
      const nextScale = frameQualityScale(qualityTier);
      if (Math.abs(nextScale - backingScale) < 0.001) return;
      backingScale = nextScale;
      resize(false);
    };
    const loop = createRenderLoop(draw, { role: "field", onQualityChange, paused });
    loopRef.current = loop;
    loop.start();

    return () => {
      loop.dispose();
      loopRef.current = null;
      window.removeEventListener("resize", onResize);
      // Backing store канваса освобождаем сразу, а не ждём сборки мусора:
      // размонтирование идёт при уходе в трей и на каждой смене темы, а в трее
      // GC не приходит. Тот же приём документирован у WebGL-сцен.
      canvas.width = 0;
      canvas.height = 0;
    };
  }, []);

  // Под reduce-motion кадр статичен, но смену состояния щита отражаем стоп-кадром.
  useEffect(() => {
    loopRef.current?.invalidate();
  }, [hot]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
  }, [paused]);

  return (
    <div className="fallen-field pointer-events-none absolute inset-0 overflow-hidden" data-hot={hot} data-fallen-motion={motionActive}>
      <FallenRuins />
      <canvas ref={ref} className="fallen-field-stars absolute inset-0 h-full w-full" style={{ imageRendering: "pixelated" }} />
      {/* Тёплое дыхание при активности — очень сдержанно. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1600ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 50%, rgba(255,120,110,0.07), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      <div className="fallen-field-vignette" />
    </div>
  );
}
