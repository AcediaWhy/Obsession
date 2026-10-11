import { useEffect, useRef, useState } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, renderStill, type QualityTier, type RenderLoop } from "../render";
import { createAuroraScene } from "./aurora/scene";
import { AuroraPointerMotion } from "./aurora/pointer";
import { subscribePointerFrame } from "../pointerBus";
import auroraPoster from "../../assets/aurora-poster.webp";
import "../../styles/aurora.css";

export function AuroraField({ paused = false }: { paused?: boolean }) {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;
  const hotRef = useRef(hot);
  hotRef.current = hot;
  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const scene = createAuroraScene(canvas);
    if (!scene) return;
    const shell = canvas.closest<HTMLElement>("[data-theme='aurora']");
    let quality: QualityTier = "high";
    let time = 12;
    let activity = hotRef.current ? 1 : 0;
    let wasHot = hotRef.current;
    let activationAge = 30;
    let reflectionTime = -1;
    const pointer = new AuroraPointerMotion();
    let pointerFrame = pointer.update(0);
    let bounds = canvas.getBoundingClientRect();
    const resize = () => {
      bounds = canvas.getBoundingClientRect();
      scene.resize(canvas.clientWidth, canvas.clientHeight, quality);
    };
    const unsubscribePointer = subscribePointerFrame((frame) => {
      if (frame.layoutChanged) { bounds = canvas.getBoundingClientRect(); return; }
      if (renderStill() || pausedRef.current) return;
      if (frame.clientX < bounds.left || frame.clientX > bounds.right
        || frame.clientY < bounds.top || frame.clientY > bounds.bottom) { pointer.leave(); return; }
      pointer.move((frame.clientX - bounds.left) / Math.max(1, bounds.width),
        1 - (frame.clientY - bounds.top) / Math.max(1, bounds.height));
    });
    const onPointerLeave = () => pointer.leave();
    window.addEventListener("blur", onPointerLeave);
    document.documentElement.addEventListener("pointerleave", onPointerLeave);
    const updateReflection = () => {
      if (!shell) return;
      shell.style.setProperty("--aurora-light-x", `${(51 + Math.sin(time * 0.065) * 24 + (pointerFrame.x - 0.5) * pointerFrame.strength * 14).toFixed(1)}%`);
      shell.style.setProperty("--aurora-light", (0.055 + activity * 0.03 + Math.sin(time * 0.09) * 0.012).toFixed(3));
      shell.dataset.auroraStill = pausedRef.current || renderStill() ? "true" : "false";
    };
    const draw = (dt: number) => {
      time += dt;
      if (hotRef.current && !wasHot) activationAge = 0;
      wasHot = hotRef.current;
      activationAge += dt;
      const still = renderStill() || pausedRef.current;
      // A still frame reflects the final shield state immediately. Animated
      // exposure otherwise eases over several seconds within the same palette.
      activity = still ? Number(hotRef.current)
        : activity + (Number(hotRef.current) - activity) * (1 - Math.exp(-dt * 0.65));
      if (still) pointer.reset();
      pointerFrame = pointer.update(still ? 0 : dt);
      scene.draw(time, activity, still ? 30 : activationAge, pointerFrame);
      canvas.dataset.ready = "true";
      if (time - reflectionTime >= 0.5 || dt === 0 || still) {
        reflectionTime = time;
        updateReflection();
      }
    };
    const loop = createRenderLoop(draw, {
      role: "field", fps: 30, paused: pausedRef.current,
      onQualityChange(nextQuality) { quality = nextQuality; resize(); },
    });
    const onResize = () => { resize(); loop.invalidate(); };
    const onLost = (event: Event) => {
      event.preventDefault();
      delete canvas.dataset.ready;
      loop.stop();
    };
    const onRestored = () => setRevision((value) => value + 1);
    resize();
    loopRef.current = loop;
    window.addEventListener("resize", onResize);
    canvas.addEventListener("webglcontextlost", onLost);
    canvas.addEventListener("webglcontextrestored", onRestored);
    loop.start();
    return () => {
      loop.dispose();
      unsubscribePointer();
      window.removeEventListener("blur", onPointerLeave);
      document.documentElement.removeEventListener("pointerleave", onPointerLeave);
      loopRef.current = null;
      window.removeEventListener("resize", onResize);
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestored);
      shell?.style.removeProperty("--aurora-light-x");
      shell?.style.removeProperty("--aurora-light");
      if (shell) delete shell.dataset.auroraStill;
      scene.dispose();
    };
  }, [revision]);

  useEffect(() => { loopRef.current?.invalidate(); }, [hot]);
  useEffect(() => { loopRef.current?.setPaused(paused); }, [paused]);

  return <div className="aurora-field" aria-hidden="true">
    <div className="aurora-field__poster" style={{ backgroundImage: `url(${auroraPoster})` }} />
    <canvas key={revision} ref={canvasRef} className="aurora-field__canvas" />
  </div>;
}
