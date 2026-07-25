import { useEffect, useRef, useState } from "react";

import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, useMotionOff } from "../render";
import { subscribePointerFrame } from "../pointerBus";
import { RainBackdrop } from "./RainFallback";
import { runHybridRainFrame, type HybridRainFramePhases } from "./rain/hybridFramePipeline";
import { RainPipeline } from "./rain/pipeline";
import { normalizeRainPointer, smoothRainValue } from "./rain/rainFramePipeline";
import { rainQualityProfile } from "./rain/quality";
import { RainSimulation, type RainDropSprites } from "./rain/simulation";
import { RainWeatherModel, type RainWeatherSnapshot } from "./rain/weather";

// Источник мира за стеклом. Статичное фото — как в оригинальном codrops-демо
// (движение дают капли/конденсат/параллакс, а не фон); видео-режим сохранён:
// поставь WORLD_IMAGE_SRC = null и верни /rain/loop.mp4 в public/rain.
const WORLD_IMAGE_SRC: string | null = "/rain/world.jpg";
const WORLD_VIDEO_SRC = "/rain/loop.mp4";

// Спрайты капли codrops (фото-рефракционная карта + маска). Кэш на модуль:
// повторный маунт и восстановление GL-контекста не перекачивают картинки.
let spritesPromise: Promise<RainDropSprites> | null = null;
function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`Rain: не загрузился спрайт ${src}`));
    image.src = src;
  });
}
function loadDropSprites(): Promise<RainDropSprites> {
  spritesPromise ??= Promise.all([
    loadImage("/rain/drop-color.png"),
    loadImage("/rain/drop-alpha.png"),
  ]).then(([dropColor, dropAlpha]) => ({ dropColor, dropAlpha }));
  return spritesPromise;
}

export default function RainHybridScene({ paused = false }: { paused?: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<ReturnType<typeof createRenderLoop> | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const dpiActive = useDpiStore((state) => state.active);
  const proxyRunning = useProxyStore((state) => state.running);
  const reducedMotion = useMotionOff();
  const stateRef = useRef({ active: false, paused, reducedMotion });
  stateRef.current = { active: dpiActive || proxyRunning, paused, reducedMotion };
  const [ready, setReady] = useState(false);
  const [fatalError, setFatalError] = useState<Error | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let disposed = false;
    let simulation: RainSimulation | null = null;
    let pipeline: RainPipeline | null = null;
    // Мир за стеклом: фото (по умолчанию) или закольцованное видео.
    // Элементы вне DOM — служат только источником текстуры.
    let glReady = false;
    let worldReady = false;
    let worldImage: HTMLImageElement | null = null;
    let video: HTMLVideoElement | null = null;
    const maybeReady = () => {
      if (glReady && worldReady && !disposed) setReady(true);
    };
    if (WORLD_IMAGE_SRC) {
      const image = new Image();
      image.onload = () => {
        if (disposed) return;
        worldImage = image;
        worldReady = true;
        pipeline?.updateWorldImage(image);
        maybeReady();
        loopRef.current?.invalidate();
      };
      image.onerror = () => {
        if (!disposed) setFatalError(new Error("Rain: фото-мир не загрузилось"));
      };
      image.src = WORLD_IMAGE_SRC;
    } else {
      video = document.createElement("video");
      videoRef.current = video;
      video.muted = true;
      video.loop = true;
      video.playsInline = true;
      video.preload = "auto";
      const syncVideoPlayback = () => {
        // play() может быть отклонён (гонка с pause) — следующий sync поправит.
        if (!video) return;
        if (stateRef.current.paused || stateRef.current.reducedMotion) video.pause();
        else video.play().catch(() => {});
      };
      video.addEventListener("loadeddata", () => {
        worldReady = true;
        maybeReady();
        syncVideoPlayback();
      });
      video.addEventListener("error", () => {
        if (!disposed) setFatalError(new Error("Rain: видео-мир не загрузилось"));
      });
      video.src = WORLD_VIDEO_SRC;
    }
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    let restoreTimer: ReturnType<typeof setTimeout> | null = null;
    let restoreAttempts = 0;
    let awaitingContextRestore = false;
    let canvasRect = canvas.getBoundingClientRect();
    let quality = rainQualityProfile("high");
    let elapsed = 0;
    const weather = new RainWeatherModel(stateRef.current.active);
    let weatherFrame: RainWeatherSnapshot = weather.step(0, {
      active: stateRef.current.active,
      reducedMotion: stateRef.current.reducedMotion,
    });
    const pendingPointer = { x: 0, y: 0 };
    const target = { x: 0, y: 0 };
    const parallax = { x: 0, y: 0 };

    const resizeScene = () => {
      resizeTimer = null;
      canvasRect = canvas.getBoundingClientRect();
      const cssWidth = canvasRect.width || window.innerWidth;
      const cssHeight = canvasRect.height || window.innerHeight;
      const width = Math.max(1, Math.round(cssWidth * quality.waterScale));
      const height = Math.max(1, Math.round(cssHeight * quality.waterScale));
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      simulation?.resize(width, height, quality.waterScale);
      pipeline?.resize(width, height, quality);
      loopRef.current?.invalidate();
    };
    resizeScene();

    const phases: HybridRainFramePhases = {
      input: () => {
        target.x = pendingPointer.x;
        target.y = pendingPointer.y;
      },
      weather: (dt) => {
        elapsed += dt;
        parallax.x = smoothRainValue(parallax.x, target.x, dt);
        parallax.y = smoothRainValue(parallax.y, target.y, dt);
        weatherFrame = weather.step(dt, {
          active: stateRef.current.active,
          reducedMotion: stateRef.current.reducedMotion || stateRef.current.paused,
        });
      },
      simulate: (dt) => simulation?.step(dt, weatherFrame),
      uploadWaterMap: () => {
        if (simulation) {
          pipeline?.updateWaterTexture(simulation.waterMap);
          pipeline?.updateMistTexture(simulation.mistMap);
        }
      },
      worldPass: () => {
        if (video) pipeline?.updateVideoTexture(video);
        pipeline?.renderWorld({
          parallaxX: parallax.x,
          parallaxY: parallax.y,
          elapsed,
          weather: weatherFrame,
          quality,
        });
      },
      compositePass: () => {
        pipeline?.renderComposite(weatherFrame.lightning, elapsed);
      },
    };
    const loop = createRenderLoop((dt) => runHybridRainFrame(phases, dt), {
      role: "field",
      paused,
      onQualityChange: (tier) => {
        quality = rainQualityProfile(tier);
        simulation?.setQuality(quality);
        resizeScene();
      },
    });
    loopRef.current = loop;
    loop.start();

    let sprites: RainDropSprites | null = null;
    const initializeScene = () => {
      if (disposed || !sprites) return;
      try {
        const nextSimulation = new RainSimulation(
          canvas.width,
          canvas.height,
          quality.waterScale,
          quality,
          sprites,
        );
        let nextPipeline: RainPipeline | null = null;
        try {
          nextPipeline = new RainPipeline(canvas, canvas.width, canvas.height, quality);
          nextPipeline.updateWaterTexture(nextSimulation.waterMap);
          nextPipeline.updateMistTexture(nextSimulation.mistMap);
          // Фото-мир мог загрузиться раньше пайплайна (или это ре-инит после
          // потери контекста) — заливаем кадр сразу.
          if (worldImage) nextPipeline.updateWorldImage(worldImage);
        } catch (error) {
          nextSimulation.destroy();
          throw error;
        }
        pipeline?.destroy();
        simulation?.destroy();
        simulation = nextSimulation;
        pipeline = nextPipeline;
        // Dev-хук: дев-харнесс (rain-dev.html?warp=1) прогревает water map.
        if (import.meta.env.DEV) {
          (window as unknown as Record<string, unknown>).__rainSim = simulation;
        }
        glReady = true;
        maybeReady();
        loop.invalidate();
      } catch (error: unknown) {
        if (disposed) return;
        setFatalError(error instanceof Error ? error : new Error(String(error)));
      }
    };
    loadDropSprites()
      .then((loaded) => {
        if (disposed) return;
        sprites = loaded;
        initializeScene();
      })
      .catch((error: unknown) => {
        if (disposed) return;
        setFatalError(error instanceof Error ? error : new Error(String(error)));
      });

    const unsubscribePointer = subscribePointerFrame((pointer) => {
      if (pointer.layoutChanged) canvasRect = canvas.getBoundingClientRect();
      const next = normalizeRainPointer(pointer.clientX, pointer.clientY, canvasRect);
      pendingPointer.x = next.x;
      pendingPointer.y = next.y;
    });
    const onResize = () => {
      if (resizeTimer != null) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(resizeScene, 120);
    };
    const clearRestoreTimer = () => {
      if (restoreTimer == null) return;
      clearTimeout(restoreTimer);
      restoreTimer = null;
    };
    const onContextLost = (event: Event) => {
      event.preventDefault();
      if (awaitingContextRestore) return;
      setReady(false);
      pipeline?.abandonAfterContextLoss();
      pipeline = null;
      simulation?.destroy();
      simulation = null;
      if (restoreAttempts >= 1) {
        setFatalError(new Error("Rain: WebGL context lost repeatedly"));
        return;
      }
      restoreAttempts += 1;
      awaitingContextRestore = true;
      restoreTimer = setTimeout(() => {
        if (!awaitingContextRestore || disposed) return;
        awaitingContextRestore = false;
        setFatalError(new Error("Rain: WebGL context restore timed out"));
      }, 2000);
    };
    const onContextRestored = () => {
      if (!awaitingContextRestore || disposed) return;
      awaitingContextRestore = false;
      clearRestoreTimer();
      initializeScene();
    };
    window.addEventListener("resize", onResize);
    canvas.addEventListener("webglcontextlost", onContextLost);
    canvas.addEventListener("webglcontextrestored", onContextRestored);

    return () => {
      disposed = true;
      loop.dispose();
      loopRef.current = null;
      if (resizeTimer != null) clearTimeout(resizeTimer);
      clearRestoreTimer();
      unsubscribePointer();
      window.removeEventListener("resize", onResize);
      canvas.removeEventListener("webglcontextlost", onContextLost);
      canvas.removeEventListener("webglcontextrestored", onContextRestored);
      pipeline?.destroy();
      simulation?.destroy();
      canvas.width = 0;
      canvas.height = 0;
      // Освобождаем видеодекодер (WebView2 не чистит detached <video> сам).
      videoRef.current = null;
      if (video) {
        video.pause();
        video.removeAttribute("src");
        video.load();
      }
      worldImage = null;
    };
  }, []);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
    const video = videoRef.current;
    if (video) {
      if (paused || reducedMotion) video.pause();
      else video.play().catch(() => {});
    }
  }, [dpiActive, proxyRunning, paused, reducedMotion]);

  if (fatalError) throw fatalError;
  return (
    <div className="pointer-events-none absolute inset-0">
      {!ready && <RainBackdrop />}
      <canvas
        ref={canvasRef}
        className="absolute inset-0 h-full w-full transition-opacity duration-500"
        style={{ opacity: ready ? 1 : 0 }}
      />
    </div>
  );
}
