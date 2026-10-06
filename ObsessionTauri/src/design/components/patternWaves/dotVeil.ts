import { createRenderLoop } from "../../render";
import { observePatternPalette, type PatternPalette } from "./theme";

type Options = { interactive: boolean; motionOff: boolean; hidden: boolean };
type Dot = { x: number; y: number; radius: number; alpha: number };

// The resting veil is transparent 2D artwork. No GL context, simulation textures
// or ambient timer: draw once, then wake only while a pointer response settles.
export function createDotVeil(host: HTMLElement, initial: Options) {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d", { alpha: true });
  if (!ctx) return { update: (_patch: Options) => {}, dispose: () => {} };
  canvas.setAttribute("aria-hidden", "true");
  canvas.dataset.renderer = "2d";
  host.appendChild(canvas);
  const card = host.closest<HTMLElement>(".z2-service-card") ?? host;
  let options = initial;
  let palette: PatternPalette;
  let width = 1, height = 1, dpr = 1;
  let dots: Dot[] = [];
  let alive = true, visible = true, running = false, last = 0;
  let rect = host.getBoundingClientRect();
  const target = { x: 0, y: 0, inside: false };
  const cursor = { x: 0, y: 0, hover: 0 };

  const wake = () => {
    if (!alive || !visible || options.hidden || document.hidden) return;
    if (!running) { last = performance.now(); running = true; }
    loop.start(); loop.setPaused(false); loop.invalidate();
  };
  const loop = createRenderLoop((_dt, now) => {
    const paintStart = performance.now();
    if (!alive || !palette || width < 2 || height < 2) { running = false; loop.setPaused(true); return; }
    const dt = Math.min(.05, Math.max(0, (now - last) / 1000));
    last = now;
    const hover = options.interactive && !options.motionOff && target.inside ? 1 : 0;
    cursor.hover += (hover - cursor.hover) * (1 - Math.exp(-dt * 7));
    cursor.x += (target.x - cursor.x) * (1 - Math.exp(-dt * 9));
    cursor.y += (target.y - cursor.y) * (1 - Math.exp(-dt * 9));
    if (options.motionOff) cursor.hover = 0;
    const settling = Math.abs(hover - cursor.hover) > .002
      || !!hover && Math.hypot(target.x - cursor.x, target.y - cursor.y) > .03;
    if (!settling) { cursor.hover = hover; cursor.x = target.x; cursor.y = target.y; }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = `rgb(${palette.color.slice(0,3).map(value => Math.round(value * 255)).join(" ")})`;
    const reach = Math.max(70, width * .23);
    for (const dot of dots) {
      const dx = dot.x - cursor.x, dy = dot.y - cursor.y;
      const pressure = Math.exp(-(dx * dx + dy * dy) / (reach * reach)) * cursor.hover;
      // A broad, subtle lens with a 2px displacement; no impulse on enter/click.
      ctx.globalAlpha = dot.alpha * (1 + pressure * .28);
      ctx.beginPath();
      ctx.arc(dot.x + dx / reach * pressure * 2, dot.y + dy / reach * pressure * 2,
        dot.radius * (1 + pressure * .10), 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
    if (import.meta.env.DEV) {
      canvas.dataset.frames = String(Number(canvas.dataset.frames || 0) + 1);
      canvas.dataset.hover = cursor.hover.toFixed(3);
      canvas.dataset.settling = String(settling);
      canvas.dataset.color = palette.color.slice(0,3).map(value => Math.round(value * 255)).join(" ");
      canvas.dataset.dots = String(dots.length);
      canvas.dataset.paintMs = (performance.now() - paintStart).toFixed(3);
    }
    running = settling;
    loop.setPaused(!settling);
  }, { role: "secondary", fps: 60, paused: true, maxDt: .05 });

  const resize = () => {
    width = Math.max(1, host.clientWidth); height = Math.min(200, Math.max(1, host.clientHeight));
    dpr = Math.min(1.25, window.devicePixelRatio || 1, 640 / width);
    canvas.width = Math.round(width * dpr); canvas.height = Math.round(height * dpr);
    rect = host.getBoundingClientRect();
    const spacing = Math.max(6.4, Math.sqrt(width * height / 1400));
    dots = [];
    for (let y = spacing / 2; y < height; y += spacing) for (let x = spacing / 2; x < width; x += spacing) {
      const u = (x - width * .58) / 160, v = (y - height * .5) / 180;
      const fold = Math.sin(u * 5.5 + Math.sin(v * 2.1) * .9 + .32 * Math.sin(u * 2.7 + v));
      const tone = .5 + .5 * fold;
      const fade = Math.exp(-Math.pow((x - width * .61) / (width * .38),2)
        - Math.pow((y - height * .5) / (height * .8),2));
      dots.push({x, y, radius: .55 + tone * 1.15, alpha: (.16 + tone * .65) * fade});
    }
    wake();
  };
  const move = (event: PointerEvent) => {
    if (!options.interactive || options.motionOff || options.hidden || !visible) return;
    rect = host.getBoundingClientRect();
    const x = event.clientX - rect.left, y = event.clientY - rect.top;
    if (!target.inside && cursor.hover < .002) { cursor.x = x; cursor.y = y; }
    target.x = x; target.y = y; target.inside = true;
    wake();
  };
  const leave = () => { if (!target.inside) return; target.inside = false; wake(); };
  card.addEventListener("pointermove", move, { passive: true });
  card.addEventListener("pointerleave", leave);
  window.addEventListener("blur", leave);
  const stopPalette = observePatternPalette(host, next => { palette = next; wake(); });
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(host);
  const intersectionObserver = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    if (visible) wake(); else { target.inside = false; cursor.hover = 0; running = false; loop.stop(); }
  });
  intersectionObserver.observe(host);
  resize();
  return {
    update(patch: Options) {
      options = patch;
      if (options.hidden) { target.inside = false; cursor.hover = 0; running = false; loop.stop(); }
      else { if (!options.interactive || options.motionOff) target.inside = false; wake(); }
    },
    dispose() {
      if (!alive) return;
      alive = false; loop.dispose(); stopPalette(); resizeObserver.disconnect(); intersectionObserver.disconnect();
      card.removeEventListener("pointermove", move); card.removeEventListener("pointerleave", leave);
      window.removeEventListener("blur", leave); canvas.remove(); canvas.width = canvas.height = 0;
    },
  };
}
