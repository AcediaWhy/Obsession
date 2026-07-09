import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { Raindrops } from "./rain/raindrops";
import { RainRenderer } from "./rain/rainRenderer";
import { createCanvas } from "./rain/random";
import { renderActive } from "../render";

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

export default function RainScene3D() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
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
    const parallax = { x: 0, y: 0 };

    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;

    Promise.all([
      loadImage(ASSETS.dropAlpha),
      loadImage(ASSETS.dropColor),
      loadImage(ASSETS.dropShine),
      loadImage(ASSETS.background),
    ])
      .then(([dropAlpha, dropColor, dropShine, bgImage]) => {
        if (disposed || !canvasRef.current) return;

        raindrops = new Raindrops(canvas.width, canvas.height, 1, dropAlpha, dropColor, {
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
        });

        // Fg — лёгкий блюр (виден в каплях-линзах): убирает высокочастотную рябь на
        // мелких каплях следа, как у авторского гладкого texture-fg; +осветление для контраста.
        // Bg — более сильный фрост на исходной яркости (тёмная матовая база стекла).
        const capW = Math.min(bgImage.naturalWidth || 1280, 1280);
        const fg = blurredCanvas(bgImage, capW, 1.2, 1.3);
        const bg = blurredCanvas(bgImage, capW, 6, 1);

        renderer = new RainRenderer(canvas, raindrops.canvas, fg, bg, dropShine, {
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
      })
      .catch((e) => console.warn("Rain: не удалось загрузить ассеты", e));

    // Параллакс за курсором (плавно, без gsap — экспоненциальное сглаживание).
    const target = { x: 0, y: 0 };
    const onMove = (e: PointerEvent) => {
      target.x = (e.clientX / canvas.width) * 2 - 1;
      target.y = (e.clientY / canvas.height) * 2 - 1;
    };
    window.addEventListener("pointermove", onMove);

    let smoothRaf = 0;
    const smooth = () => {
      // Окно скрыто/в трее — не гоняем параллакс, держим только rAF живым.
      if (!renderActive()) {
        smoothRaf = requestAnimationFrame(smooth);
        return;
      }
      parallax.x += (target.x - parallax.x) * 0.06;
      parallax.y += (target.y - parallax.y) * 0.06;
      if (renderer) {
        renderer.parallaxX = parallax.x;
        renderer.parallaxY = parallax.y;
        // «Гроза» при активном обходе: капли крупнее/чаще.
        raindrops!.options.rainChance = hotRef.current ? 0.5 : 0.3;
        raindrops!.options.rainLimit = hotRef.current ? 16 : 10;
      }
      smoothRaf = requestAnimationFrame(smooth);
    };
    smoothRaf = requestAnimationFrame(smooth);

    const onResize = () => {
      canvas.width = window.innerWidth;
      canvas.height = window.innerHeight;
    };
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      cancelAnimationFrame(smoothRaf);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("resize", onResize);
      renderer?.destroy();
      raindrops?.destroy();
    };
  }, []);

  return (
    <div className="absolute inset-0" style={{ pointerEvents: "none" }}>
      <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
    </div>
  );
}
