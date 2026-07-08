import { useEffect, useRef, type ReactNode } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { renderActive } from "../render";
import { DepthRenderer } from "./depth/depthRenderer";

// Живые обои из фото + карты глубины (2.5D depth-parallax, как «глубинные» обои
// Wallpaper Engine). Фон — не собранная вручную геометрия, а фотография,
// «оживлённая» параллаксом за курсором. `children` рисуются поверх канвы —
// туда кладём генеративный снег/грейд (гибрид фото + процедурной атмосферы).
//
// Деградация: нет WebGL → остаётся статичное фото <img>. Отсутствие ассетов
// проверяет вызывающий (см. RussiaHybrid) и туда мы просто не заходим.

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

// Приблизительная карта глубины, когда настоящей ещё нет: вертикальный градиент
// (низ = близко/светлое, верх = далеко/тёмное). Для пейзажа/тропы даёт правдо-
// подобный вертикальный параллакс — временно, до укладки реальной depth.png.
function gradientDepth(): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = 8;
  c.height = 256;
  const cc = c.getContext("2d")!;
  const g = cc.createLinearGradient(0, 0, 0, 256);
  g.addColorStop(0, "#000");
  g.addColorStop(1, "#fff");
  cc.fillStyle = g;
  cc.fillRect(0, 0, 8, 256);
  return c;
}

export default function DepthScene({
  photoSrc,
  depthSrc,
  scale = 42,
  focus = 0.5,
  invert = false,
  children,
}: {
  photoSrc: string;
  depthSrc: string;
  scale?: number;
  focus?: number;
  invert?: boolean;
  children?: ReactNode;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hotRef = useRef(false);
  hotRef.current = dpiActive || proxyRunning;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    let renderer: DepthRenderer | null = null;
    let disposed = false;
    let raf = 0;
    let t = 0;
    let last = 0;

    const target = { x: 0, y: 0 };
    const parallax = { x: 0, y: 0 };

    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;

    // Фото обязательно; карта глубины опциональна — без неё берём синтетический
    // градиент, чтобы параллакс работал уже сейчас (реальная depth.png точнее).
    loadImage(photoSrc)
      .then(async (photo) => {
        if (disposed || !canvasRef.current) return;
        const depth = await loadImage(depthSrc).catch(() => gradientDepth());
        try {
          renderer = new DepthRenderer(canvas, photo, depth, { scale, focus, invert });
        } catch (e) {
          // WebGL недоступен — остаётся статичное фото <img> под канвой.
          console.warn("Depth: WebGL недоступен, показываю статичное фото", e);
        }
      })
      .catch((e) => console.warn("Depth: фото не загрузилось", e));

    const onMove = (e: PointerEvent) => {
      target.x = (e.clientX / canvas.width) * 2 - 1;
      target.y = (e.clientY / canvas.height) * 2 - 1;
    };
    window.addEventListener("pointermove", onMove);

    const onResize = () => {
      canvas.width = window.innerWidth;
      canvas.height = window.innerHeight;
      renderer?.resize(canvas.width, canvas.height);
    };
    window.addEventListener("resize", onResize);

    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      if (!renderer) return;
      if (!renderActive()) {
        last = 0;
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.05) : 0.016;
      last = now;
      t += dt;

      const amp = hotRef.current ? 1.15 : 1;
      parallax.x += (target.x * amp - parallax.x) * 0.06;
      parallax.y += (target.y * amp - parallax.y) * 0.06;
      renderer.parallaxX = parallax.x;
      renderer.parallaxY = parallax.y;
      renderer.render(t);
    };
    raf = requestAnimationFrame(loop);

    return () => {
      disposed = true;
      cancelAnimationFrame(raf);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("resize", onResize);
    };
  }, [photoSrc, depthSrc, scale, focus, invert]);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* База: статичное фото — видно даже без WebGL. Канва параллакса поверх. */}
      <img src={photoSrc} alt="" className="absolute inset-0 h-full w-full object-cover" />
      <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
      {/* Оверлеи (снег, грейд) — поверх всего. */}
      {children}
    </div>
  );
}
