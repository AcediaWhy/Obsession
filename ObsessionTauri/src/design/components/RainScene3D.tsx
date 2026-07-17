import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { Raindrops } from "./rain/raindrops";
import { RainRenderer } from "./rain/rainRenderer";
import {
  normalizeRainPointer,
  runRainFrame,
  smoothRainValue,
} from "./rain/rainFramePipeline";
import { createCanvas } from "./rain/random";
import { createRenderLoop, frameQualityScale } from "../render";
import { subscribePointerFrame } from "../pointerBus";

// Тема «Rain»: дождь на стекле — порт codrops/RainEffect (vanilla WebGL, без R3F).
// CPU-симуляция капель (raindrops) пишет water map, шейдер water.frag преломляет
// через неё фон (статичное фото города, резкий Fg + размытый Bg + shine-блик).
// Конфигурация — из демо index2 (статичный фон): крупные медленные капли.

const ASSETS = {
  dropAlpha: "/rain/drop-alpha.png",
  dropColor: "/rain/drop-color.png",
  dropShine: "/rain/drop-shine2.png",
  // Фон за стеклом. Замени этот файл на своё фото — движок не меняется.
  background: "/rain/bg.jpg",
};

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

// Из одного фото делаем две версии: резкую (видна в каплях, `fg`) и размытую
// (матовое стекло-база, `bg`). Небольшой оверскан убирает прозрачную кромку блюра.
function blurredCanvas(
  img: HTMLImageElement,
  targetW: number,
  blurPx: number,
  brightness = 1,
): HTMLCanvasElement {
  const ratio = img.naturalHeight / img.naturalWidth || 0.66;
  const w = targetW;
  const h = Math.round(targetW * ratio);
  const c = createCanvas(w, h);
  const ctx = c.getContext("2d")!;
  ctx.filter = `blur(${blurPx}px) brightness(${brightness})`;
  const o = blurPx * 2; // оверскан
  ctx.drawImage(img, -o, -o, w + o * 2, h + o * 2);
  ctx.filter = "none";
  return c;
}

export default function RainScene3D({ paused = false }: { paused?: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const frameLoopRef = useRef<ReturnType<typeof createRenderLoop> | null>(null);
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hotRef = useRef(false);
  hotRef.current = dpiActive || proxyRunning;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    let raindrops: Raindrops | null = null;
    let renderer: RainRenderer | null = null;
    let disposed = false;
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    let canvasRect = canvas.getBoundingClientRect();
    let qualityScale = 1;
    let frameLoop: ReturnType<typeof createRenderLoop> | null = null;
    const pendingPointer = { x: 0, y: 0 };
    const target = { x: 0, y: 0 };
    const parallax = { x: 0, y: 0 };

    const resizeScene = () => {
      resizeTimer = null;
      canvasRect = canvas.getBoundingClientRect();
      const cssWidth = canvasRect.width || window.innerWidth;
      const cssHeight = canvasRect.height || window.innerHeight;
      const width = Math.max(1, Math.round(cssWidth * qualityScale));
      const height = Math.max(1, Math.round(cssHeight * qualityScale));
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      if (raindrops) {
        raindrops.scale = qualityScale;
        raindrops.resize(width, height);
      }
      renderer?.resize(width, height);
      frameLoop?.invalidate();
    };
    resizeScene();

    const phases = {
      input: () => {
        target.x = pendingPointer.x;
        target.y = pendingPointer.y;
        if (!raindrops) return;
        raindrops.options.rainChance = hotRef.current ? 0.5 : 0.3;
        raindrops.options.rainLimit = hotRef.current ? 16 : 10;
      },
      smooth: (dt: number) => {
        parallax.x = smoothRainValue(parallax.x, target.x, dt);
        parallax.y = smoothRainValue(parallax.y, target.y, dt);
        if (!renderer) return;
        renderer.parallaxX = parallax.x;
        renderer.parallaxY = parallax.y;
      },
      simulate: (dt: number) => raindrops?.step(dt),
      uploadTexture: () => renderer?.updateTexture(),
      draw: () => renderer?.draw(),
    };
    frameLoop = createRenderLoop((dt) => runRainFrame(phases, dt), {
      role: "field",
      paused,
      onQualityChange: (qualityTier) => {
        qualityScale = frameQualityScale(qualityTier);
        resizeScene();
      },
    });
    frameLoopRef.current = frameLoop;
    frameLoop.start();

    Promise.all([
      loadImage(ASSETS.dropAlpha),
      loadImage(ASSETS.dropColor),
      loadImage(ASSETS.dropShine),
      loadImage(ASSETS.background),
    ])
      .then(([dropAlpha, dropColor, dropShine, bgImage]) => {
        if (disposed || !canvasRef.current) return;

        const nextRaindrops = new Raindrops(
          canvas.width,
          canvas.height,
          qualityScale,
          dropAlpha,
          dropColor,
          {
            minR: 14,
            maxR: 44,
            rainChance: 0.3,
            rainLimit: 10,
            dropletsRate: 0,
            globalTimeScale: 0.45,
            trailRate: 1.1,
            dropFallMultiplier: 0.2,
            trailScaleRange: [0.2, 0.35],
            autoShrink: false,
            spawnArea: [-0.3, 0.3],
            collisionRadius: 0.45,
            collisionRadiusIncrease: 0,
            collisionBoost: 0.35,
            collisionBoostMultiplier: 0.025,
          },
        );

        // Fg — лёгкий блюр (виден в каплях-линзах): убирает высокочастотную рябь на
        // мелких каплях следа, как у авторского гладкого texture-fg; +осветление для контраста.
        // Bg — более сильный фрост на исходной яркости (тёмная матовая база стекла).
        const capW = Math.min(bgImage.naturalWidth || 1280, 1280);
        const fg = blurredCanvas(bgImage, capW, 1.2, 1.3);
        const bg = blurredCanvas(bgImage, capW, 6, 1);
        let nextRenderer: RainRenderer;
        try {
          nextRenderer = new RainRenderer(canvas, nextRaindrops.canvas, fg, bg, dropShine, {
            // Тень капель выключена — на тёмном фото она делает капли мутными.
            // brightness множит только содержимое капель → они «выстреливают»
            // на тёмной матовой базе, читаясь как чистое стекло.
            renderShadow: false,
            minRefraction: 195,
            maxRefraction: 512,
            brightness: 1.25,
            alphaMultiply: 7,
            alphaSubtract: 3,
          });
        } catch (error) {
          nextRaindrops.destroy();
          throw error;
        }

        raindrops = nextRaindrops;
        renderer = nextRenderer;
        frameLoop?.invalidate();
      })
      .catch((error) => {
        if (!disposed) console.warn("Rain: не удалось загрузить ассеты", error);
      });

    // Pointer bus отдаёт только последний sample за frame. При layout change
    // обновляем cached rect до нормализации координат сцены.
    const unsubscribePointer = subscribePointerFrame((pointer) => {
      if (pointer.layoutChanged) canvasRect = canvas.getBoundingClientRect();
      const next = normalizeRainPointer(pointer.clientX, pointer.clientY, canvasRect);
      pendingPointer.x = next.x;
      pendingPointer.y = next.y;
    });

    // Resize WebGL и water-map только после короткой осадки window resize.
    const onResize = () => {
      if (resizeTimer != null) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(resizeScene, 120);
    };
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      frameLoop?.dispose();
      frameLoopRef.current = null;
      if (resizeTimer != null) clearTimeout(resizeTimer);
      unsubscribePointer();
      window.removeEventListener("resize", onResize);
      renderer?.destroy();
      raindrops?.destroy();
    };
  }, []);

  useEffect(() => {
    frameLoopRef.current?.setPaused(paused);
  }, [paused]);

  return (
    <div className="absolute inset-0" style={{ pointerEvents: "none" }}>
      <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
    </div>
  );
}
