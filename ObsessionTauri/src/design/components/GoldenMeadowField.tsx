import { useEffect, useLayoutEffect, useRef } from "react";
import { createGoldenMeadowRenderer } from "../../labs/goldenMeadowRenderer";
import { createMeadowInteraction } from "../../labs/goldenMeadowInteraction.js";
import { createRenderLoop, useMotionOff, type RenderLoop } from "../render";
import posterUrl from "../../assets/golden-meadow-poster.webp";
import "../../styles/goldenMeadow.css";

// Warm the small poster while the app starts, before the user selects the theme.
if (typeof Image !== "undefined") {
  const poster = new Image();
  poster.decoding = "async";
  poster.src = posterUrl;
}

export function GoldenMeadowField({ paused = false }: { paused?: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const pausedRef = useRef(paused);
  const motionOff = useMotionOff();
  const motionOffRef = useRef(motionOff);
  pausedRef.current = paused;
  motionOffRef.current = motionOff;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    // A fresh element per effect also supports React StrictMode: a transferred
    // canvas cannot be transferred again after the first worker is terminated.
    const canvas = document.createElement("canvas");
    canvas.className = "block h-full w-full object-cover";
    canvas.style.opacity = "0";
    host.append(canvas);
    let disposed = false;
    let failed = false;
    let renderer: ReturnType<typeof createGoldenMeadowRenderer> | undefined;
    let fallbackTimer: ReturnType<typeof setTimeout> | undefined;
    const fallback = (message: string) => {
      if (disposed || failed) return;
      failed = true;
      console.warn("Golden Meadow uses a still fallback:", message);
      renderer?.dispose();
      loopRef.current?.stop();
      loopRef.current = null;
      canvas.remove();
      // Older WebViews without OffscreenCanvas get one small still frame, not
      // the expensive continuous renderer on the UI thread.
      fallbackTimer = setTimeout(() => {
        void import("../../labs/goldenMeadowScene.js").then(({ createGoldenMeadow }) => {
          if (disposed) return;
          const still = document.createElement("canvas");
          still.className = canvas.className;
          host.append(still);
          const scene = createGoldenMeadow(still, { scale: 1 });
          scene.render(0);
          scene.dispose();
          still.dataset.renderer = "still-fallback";
        }).catch((error) => console.warn("Golden Meadow fallback failed:", error));
      }, 450);
    };
    try { renderer = createGoldenMeadowRenderer(canvas, fallback); }
    catch (error) { fallback(String(error)); }

    const interaction = createMeadowInteraction();
    let time = 0;
    const loop = createRenderLoop((dt) => {
      time += dt;
      renderer?.render(time, { wind: 1, windTime: time, ...interaction.sample(time) });
    }, { role: "field", fps: 30 });
    loopRef.current = failed ? null : loop;
    if (!pausedRef.current && !failed) loop.start();

    let bounds = host.getBoundingClientRect();
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const updateBounds = () => {
      bounds = host.getBoundingClientRect();
      clearTimeout(resizeTimer);
      if (!bounds.width || !bounds.height) return;
      resizeTimer = setTimeout(() => renderer?.resize(bounds.width, bounds.height), 180);
    };
    const observer = new ResizeObserver(updateBounds);
    observer.observe(host);
    window.addEventListener("resize", updateBounds);
    const onPointerMove = (event: PointerEvent) => {
      if (!event.isPrimary || event.pointerType === "touch" || pausedRef.current || motionOffRef.current || document.hidden) return;
      if (!bounds.width || !bounds.height) return;
      const scale = Math.max(bounds.width / 735, bounds.height / 505);
      const left = (bounds.width - 735 * scale) / 2;
      const top = (bounds.height - 505 * scale) / 2;
      interaction.move((event.clientX - bounds.left - left) / scale,
        (event.clientY - bounds.top - top) / scale, time, event.pointerId);
    };
    const reset = () => interaction.reset();
    window.addEventListener("pointermove", onPointerMove, { passive: true });
    window.addEventListener("pointercancel", reset);
    window.addEventListener("blur", reset);
    return () => {
      disposed = true;
      loop.dispose();
      loopRef.current = null;
      clearTimeout(resizeTimer);
      clearTimeout(fallbackTimer);
      observer.disconnect();
      window.removeEventListener("resize", updateBounds);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointercancel", reset);
      window.removeEventListener("blur", reset);
      renderer?.dispose();
      host.replaceChildren();
    };
  }, []);

  useLayoutEffect(() => {
    if (paused) loopRef.current?.stop();
    else loopRef.current?.start();
  }, [paused]);

  return <div ref={hostRef} className="absolute inset-0 overflow-hidden bg-[#b87b25]" style={{ contain: "strict", backgroundImage: `url(${posterUrl})`, backgroundPosition: "center", backgroundSize: "cover" }} aria-hidden="true" />;
}
