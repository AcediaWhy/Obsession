import { useEffect, useRef } from "react";

import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import {
  createRenderLoop,
  useMotionOff,
  type QualityTier,
  type RenderLoop,
} from "../render";
import { rainQualityProfile } from "./rain/quality";
import { RainWeatherModel, type RainWeatherSnapshot } from "./rain/weather";

/** Мгновенная процедурная подложка темы Rain без изображений. Крупные мягкие
 *  градиенты повторяют тональную карту WebGL-сцены, пока шейдер компилируется,
 *  и остаются атмосферным фоном 2D-фолбэка. */
export function RainBackdrop() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden bg-[#04060a]">
      <div
        className="absolute inset-0"
        style={{
          background:
            "radial-gradient(80% 46% at 69% 48%, rgba(127,88,45,0.16), transparent 48%), radial-gradient(95% 58% at 52% 52%, rgba(83,112,113,0.34), transparent 62%), linear-gradient(180deg, #071017 0%, #1d2d30 48%, #0b1a1b 62%, #03090a 100%)",
        }}
      />
      <div
        className="absolute inset-x-[-8%] top-[30%] h-[31%] blur-[7px]"
        style={{
          background:
            "radial-gradient(34% 74% at 8% 100%, rgba(5,17,16,0.96), transparent 70%), radial-gradient(26% 68% at 94% 100%, rgba(4,14,13,0.94), transparent 72%), radial-gradient(64% 46% at 50% 100%, rgba(19,40,39,0.94), transparent 76%)",
        }}
      />
      <div
        className="absolute inset-x-0 top-[55.5%] bottom-0"
        style={{
          background:
            "linear-gradient(180deg, rgba(70,105,106,0.44) 0%, rgba(16,42,43,0.82) 32%, rgba(3,12,13,0.98) 100%)",
        }}
      />
      <div
        className="absolute inset-x-0 top-[56%] h-[31%] opacity-30"
        style={{
          backgroundImage:
            "repeating-linear-gradient(178deg, transparent 0 7px, rgba(139,171,166,0.13) 8px, transparent 9px 15px)",
          maskImage: "linear-gradient(to bottom, black, transparent)",
          WebkitMaskImage: "linear-gradient(to bottom, black, transparent)",
        }}
      />
      <div
        className="absolute inset-x-[-10%] top-[48%] h-[18%] blur-xl"
        style={{
          background:
            "radial-gradient(ellipse at center, rgba(105,139,138,0.34), rgba(73,102,103,0.14) 45%, transparent 76%)",
        }}
      />
      <div className="absolute inset-0 bg-[radial-gradient(120%_105%_at_50%_44%,transparent_38%,rgba(2,4,8,0.62)_100%)]" />
    </div>
  );
}

type FallbackDrop = { x: number; y: number; radius: number; speed: number; alpha: number };

/** 2D-фолбэк темы Rain без WebGL: процедурная подложка + капли и зарницы,
 *  нарисованные Canvas2D поверх. */
export function RainFallback({ paused = false }: { paused?: boolean }) {
  const dpiActive = useDpiStore((state) => state.active);
  const proxyRunning = useProxyStore((state) => state.running);
  const reducedMotion = useMotionOff();
  const stateRef = useRef({ active: dpiActive || proxyRunning, paused, reducedMotion });
  stateRef.current = { active: dpiActive || proxyRunning, paused, reducedMotion };
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const weather = new RainWeatherModel(stateRef.current.active);
    let weatherFrame: RainWeatherSnapshot = weather.step(0, {
      active: stateRef.current.active,
      reducedMotion: stateRef.current.reducedMotion,
    });
    let drops: FallbackDrop[] = [];
    let width = 0;
    let height = 0;
    let scale = 0.75;
    let quality: QualityTier = "high";
    let spawnAccumulator = 0;

    // maxDrops теперь в шкале codrops (900/600/350) — делитель держит прежнюю
    // плотность 2D-фолбэка (~20/13/8 капель по тирам).
    const dropCap = () => Math.min(20, Math.round(rainQualityProfile(quality).maxDrops / 45));
    const spawnDrop = (): FallbackDrop => ({
      x: Math.random() * width,
      y: Math.random() * height * 0.4,
      radius: 1.6 + Math.random() * 4.4,
      speed: 14 + Math.random() * 30,
      alpha: 0.14 + Math.random() * 0.24,
    });
    const resize = () => {
      width = Math.max(1, canvas.clientWidth);
      height = Math.max(1, canvas.clientHeight);
      canvas.width = Math.max(1, Math.round(width * scale));
      canvas.height = Math.max(1, Math.round(height * scale));
      ctx.setTransform(scale, 0, 0, scale, 0, 0);
      drops = Array.from({ length: Math.min(9, dropCap()) }, spawnDrop);
    };
    resize();

    const drawDrop = (drop: FallbackDrop) => {
      const gradient = ctx.createRadialGradient(
        drop.x - drop.radius * 0.25,
        drop.y - drop.radius * 0.3,
        drop.radius * 0.08,
        drop.x,
        drop.y,
        drop.radius,
      );
      gradient.addColorStop(0, `rgba(214,228,232,${drop.alpha})`);
      gradient.addColorStop(0.4, `rgba(150,166,170,${drop.alpha * 0.14})`);
      gradient.addColorStop(1, `rgba(190,206,210,${drop.alpha * 0.4})`);
      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.ellipse(drop.x, drop.y, drop.radius, drop.radius * 1.45, -0.1, 0, Math.PI * 2);
      ctx.fill();
    };

    const draw = (dt: number) => {
      const state = stateRef.current;
      weatherFrame = weather.step(dt, {
        active: state.active,
        reducedMotion: state.reducedMotion || state.paused,
      });
      if (!state.paused && !state.reducedMotion) {
        spawnAccumulator += dt * (1.0 + weatherFrame.activity * 1.3);
        if (spawnAccumulator >= 1 && drops.length < dropCap()) {
          spawnAccumulator -= 1;
          drops.push(spawnDrop());
        }
        for (const drop of drops) {
          drop.y += drop.speed * weatherFrame.rainSpeed * dt;
          drop.x -= drop.speed * weatherFrame.wind * 0.3 * dt;
          if (drop.y > height + drop.radius || drop.x < -drop.radius) {
            drop.y = -drop.radius;
            drop.x = Math.random() * width * 1.05;
          }
        }
      }
      // Канвас прозрачный: постер-подложка просвечивает, рисуем только воду.
      ctx.clearRect(0, 0, width, height);
      for (const drop of drops) drawDrop(drop);
      if (weatherFrame.lightning > 0) {
        ctx.fillStyle = `rgba(186,212,236,${weatherFrame.lightning * 0.6})`;
        ctx.fillRect(0, 0, width, height);
      }
    };

    const onResize = () => {
      resize();
      loopRef.current?.invalidate();
    };
    const loop = createRenderLoop(draw, {
      role: "field",
      paused,
      onQualityChange: (nextQuality) => {
        quality = nextQuality;
        scale = rainQualityProfile(nextQuality).waterScale;
        resize();
      },
    });
    loopRef.current = loop;
    window.addEventListener("resize", onResize);
    loop.start();
    return () => {
      loop.dispose();
      loopRef.current = null;
      drops = [];
      window.removeEventListener("resize", onResize);
      canvas.width = 0;
      canvas.height = 0;
    };
  }, []);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [dpiActive, proxyRunning, paused, reducedMotion]);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      <RainBackdrop />
      <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
    </div>
  );
}
