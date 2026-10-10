import { useId, useLayoutEffect, useRef, type CSSProperties } from "react";
import brandArtwork from "../../../src-tauri/icons/icon.png";
import type { Theme } from "../../store/themeStore";
import { THEMES } from "../../store/themeStore";
import { useMotionOff, useRenderHidden } from "../render";
import { STARTUP_INTRO_DURATION, STARTUP_QUICK_DURATION, useStartupIntro } from "../startupIntro";
import { GoldenMeadowCore } from "./GoldenMeadowCore";
import "../../styles/startupIntro.css";

const THEME_COLORS: Record<Theme, { accent: string; surface: string }> = {
  goldenmeadow: { accent: "#e6bc77", surface: "#171610" },
  aurora: { accent: "#84d9ce", surface: "#10181b" },
  ophanim: { accent: "#ceb5df", surface: "#19151e" },
  japan: { accent: "#a8c6dd", surface: "#141a21" },
  midnight: { accent: "#c8b399", surface: "#16181d" },
  catnap: { accent: "#ddb7ce", surface: "#1c1721" },
  fallendown: { accent: "#e0d5b7", surface: "#19181d" },
  yanineko: { accent: "#b7ce9b", surface: "#161c16" },
};
const SETTLE = "cubic-bezier(.22,1,.36,1)";
const ARRIVAL_AT = 2050;
const APERTURE_AT = 1940;
const APERTURE_END = 3200;
const DOT_IMPACT = 1010;
const DOT_SETTLED = 1640;

// Sample parabolic flights once; WAAPI interpolates them on the compositor.
// Position and squash live on separate elements so the arc does not wobble.
function dotFlight(fontSize: number, startX: number): Keyframe[] {
  const frames: Keyframe[] = [];
  const pose = (time: number, x: number, y: number) => {
    const frame = { offset: time / DOT_SETTLED, transform: `translate(${x}px,${y}px)` };
    if (frames[frames.length - 1]?.offset === frame.offset) frames.pop();
    frames.push(frame);
  };
  const arc = (start: number, end: number, x0: number, y0: number, x1: number, y1: number, height: number) => {
    const steps = Math.ceil((end - start) / 12);
    for (let step = 0; step <= steps; step++) {
      const t = step / steps;
      pose(start + (end - start) * t, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t - 4 * height * t * (1 - t));
    }
  };
  pose(0, startX, fontSize * .666);
  pose(160, startX, fontSize * .666);
  pose(220, startX, fontSize * .70);
  arc(240, DOT_IMPACT, startX, fontSize * .666, 0, fontSize * .065, fontSize * 1.45);
  pose(1045, 0, fontSize * .145);
  arc(1045, 1370, 0, fontSize * .145, fontSize * .055, fontSize * .075, fontSize * .29);
  pose(1395, fontSize * .055, fontSize * .09);
  arc(1395, 1560, fontSize * .055, fontSize * .09, 0, 0, fontSize * .075);
  pose(DOT_SETTLED, 0, 0);
  return frames;
}

/** The O counter becomes a window into the existing scene. */
export function StartupIntro({ theme }: { theme: Theme }) {
  const apertureId = useId();
  const request = useStartupIntro((s) => s.request);
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  const rootRef = useRef<HTMLDivElement>(null);
  const skipRef = useRef<HTMLButtonElement>(null);
  const playback = useRef({ id: 0, started: false });

  useLayoutEffect(() => {
    if (!request) return;
    if (playback.current.id !== request.id) playback.current = { id: request.id, started: false };
    const { finish, arrive } = useStartupIntro.getState();
    // Launching into the tray waits for the first visible frame. Hiding a
    // running sequence cancels it without consuming the first-launch marker.
    if (hidden && !playback.current.started && !motionOff) return;
    if (motionOff || hidden) { finish(request.id, !hidden); return; }
    const root = rootRef.current;
    if (!root) return;
    playback.current.started = true;
    const shell = root.closest<HTMLElement>("[data-startup-shell]");
    const content = shell?.querySelector<HTMLElement>("[data-startup-content]");
    const previouslyInert = content?.hasAttribute("inert");
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    content?.setAttribute("inert", "");
    if (request.mode === "full") root.focus({ preventScroll: true });
    const animations: Animation[] = [];
    const timers: number[] = [];
    let disposed = false;
    const animate = (element: Element | null, frames: Keyframe[], duration: number, delay = 0, easing = SETTLE) => {
      if (element) animations.push(element.animate(frames, { duration, delay, easing, fill: "both" }));
    };
    const end = () => finish(request.id);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); end(); }
      if (event.key === "Tab" && request.mode === "full") { event.preventDefault(); skipRef.current?.focus(); }
    };
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("resize", end);

    if (request.mode === "quick") {
      animate(root.querySelector(".startup-quick-veil"), [{ opacity: 1 }, { opacity: 0 }], STARTUP_QUICK_DURATION);
      arrive(request.id);
      timers.push(window.setTimeout(end, STARTUP_QUICK_DURATION));
    } else {
      const startFull = () => {
        if (disposed || useStartupIntro.getState().request?.id !== request.id) return;
        const logo = root.querySelector<HTMLElement>(".startup-logo")!;
        const title = root.querySelector<HTMLElement>(".startup-wordmark")!;
        const titleBox = title.getBoundingClientRect();
        const fontSize = parseFloat(getComputedStyle(title).fontSize);
        // Each letter has a gesture, but shares one settling rhythm. These
        // outer transforms remain independent of the later impact reactions.
        const entrances = [
          ["translate(-.22em,.48em) rotate(-32deg) scale(.78)", "translate(0,-.025em) rotate(3deg) scale(1.03)", 90],
          ["translateY(.85em) scale(.74,1.2)", "translateY(-.035em) scale(1.045,.97)", 160],
          ["translate(.12em,.65em) rotate(19deg)", "rotate(-2deg)", 215],
          ["translateY(.7em) rotate(-20deg) scale(.9)", "rotate(2deg) scale(1.02)", 260],
          ["translate(-.1em,.65em) rotate(-18deg)", "rotate(2deg)", 300],
          ["translate(.1em,.65em) rotate(18deg)", "rotate(-2deg)", 345],
          ["translateY(.7em) scale(.7,1.25)", "scale(1.05,.96)", 380],
          ["translate(.16em,.55em) rotate(28deg) scale(.82)", "rotate(-3deg) scale(1.025)", 405],
          ["translateY(.7em) skewX(-14deg) scale(.82,1.12)", "skewX(2deg) scale(1.03,.98)", 435],
        ] as const;
        root.querySelectorAll(".startup-letter").forEach((letter, index) => {
          const [from, overshoot, delay] = entrances[index];
          animate(letter, [
            { transform: from, opacity: 0, easing: SETTLE },
            { transform: overshoot, opacity: 1, offset: .7, easing: SETTLE },
            { transform: "none", opacity: 1 },
          ], 560, delay, "linear");
        });
        animate(root.querySelector(".startup-logo-art"), [
          { transform: "translateY(112%)" }, { transform: "translateY(0)" },
        ], 660, 300);

        // Split the actual font glyph at its inter-dot gap instead of drawing
        // a substitute circle. The final silhouette is exactly the original i.
        const iBox = root.querySelector<HTMLElement>(".startup-letter-i")!.getBoundingClientRect();
        const firstBox = root.querySelector<HTMLElement>(".startup-letter-clip")!.getBoundingClientRect();
        const dot = root.querySelector<HTMLElement>(".startup-tittle")!;
        Object.assign(dot.style, { left: `${iBox.left - titleBox.left}px`, width: `${iBox.width}px` });
        const startX = firstBox.left + firstBox.width / 2 - iBox.left - iBox.width / 2;
        animate(dot, dotFlight(fontSize, startX), DOT_SETTLED, 0, "linear");
        animate(dot, [{ opacity: 0 }, { opacity: 1 }], 150, 30);
        const squash: [number, number, number][] = [
          [0, .6, .6], [100, 1.35, 1.35], [220, 1.65, .8], [260, .85, 1.8],
          [530, 1.15, 1.05], [850, .92, 1.25], [1010, .82, 1.38], [1045, 1.65, .62],
          [1110, .84, 1.27], [1210, 1.05, .98], [1370, .93, 1.15], [1395, 1.28, .82],
          [1460, .97, 1.06], [1560, 1.05, .95], [DOT_SETTLED, 1, 1],
        ];
        animate(root.querySelector(".startup-tittle-squash"), squash.map(([time, x, y]) => ({
          offset: time / DOT_SETTLED, transform: `scale(${x},${y})`,
        })), DOT_SETTLED, 0, "linear");
        animate(root.querySelector(".startup-letter-i .startup-glyph"), [
          { transform: "none", offset: 0 },
          { transform: "scale(1.14,.88)", offset: .09, easing: SETTLE },
          { transform: "scale(.97,1.035)", offset: .48, easing: SETTLE },
          { transform: "none", offset: 1 },
        ], 400, DOT_IMPACT, "linear");
        root.querySelectorAll<HTMLElement>(".startup-letter-clip").forEach((letter, index) => {
          const distance = Math.abs(index - 6);
          if (distance < 1 || distance > 2) return;
          const lean = (index < 6 ? 1 : -1) * 3.2 / distance;
          animate(letter.querySelector(".startup-glyph"), [
            { transform: "none", offset: 0 },
            { transform: `translateY(${3 / distance}px) rotate(${lean}deg)`, offset: .16, easing: SETTLE },
            { transform: `translateY(-.5px) rotate(${-lean * .18}deg)`, offset: .58, easing: SETTLE },
            { transform: "none", offset: 1 },
          ], 460, DOT_IMPACT + distance * 45, "linear");
        });
        animate(root.querySelector(".startup-logo-clip"), [
          { transform: "none", offset: 0 },
          { transform: "translateY(-2px) rotate(3deg)", offset: .2, easing: SETTLE },
          { transform: "rotate(-.6deg)", offset: .6, easing: SETTLE },
          { transform: "none", offset: 1 },
        ], 520, DOT_IMPACT + 40, "linear");
        animate(root.querySelector(".startup-caption"), [
          { opacity: 0, transform: "translateY(8px)", easing: SETTLE },
          { opacity: 1, transform: "translateY(0)", offset: .3 },
          { opacity: 1, transform: "translateY(0)", offset: .75, easing: SETTLE },
          { opacity: 0, transform: "translateY(-8px)" },
        ], 700, 1240, "linear");

        // Match the counter of IBM Plex Sans O, then let that single shape
        // become the viewport. No copied theme image or second scene renderer.
        const rootBox = root.getBoundingClientRect();
        const aperture = root.querySelector<SVGSVGElement>(".startup-aperture")!;
        aperture.setAttribute("viewBox", `0 0 ${rootBox.width} ${rootBox.height}`);
        const cx = firstBox.left - rootBox.left + fontSize * .355;
        const cy = titleBox.top - rootBox.top + fontSize * .585;
        const width = fontSize * .456;
        const height = fontSize * .608;
        const shape = (x: number, y: number, w: number, h: number, radius: number): Keyframe => ({
          x: `${x}px`, y: `${y}px`, width: `${w}px`, height: `${h}px`, rx: `${radius}px`,
        });
        const origin = shape(cx - width / 2, cy - height / 2, width, height, width / 2);
        const apertureFrames: Keyframe[] = [
          { ...origin, offset: 0, easing: "cubic-bezier(.45,0,.7,.4)" },
        ];
        // Sample a continuous curve: round at birth, wider through the middle,
        // square only as it meets the window. Zero endpoint velocity avoids a
        // sudden rectangular jump or a hard stop at the edges.
        for (let step = 0; step <= 64; step++) {
          const t = step / 64;
          const p = t * t * t * (10 + t * (-15 + 6 * t));
          const w = width * 1.16 + (rootBox.width + 16 - width * 1.16) * p;
          const h = height * 1.08 + (rootBox.height + 16 - height * 1.08) * p;
          const x = (cx - width * .58) * (1 - p) - 8 * p;
          const y = (cy - height * .54) * (1 - p) - 8 * p;
          const radius = Math.min(w, h) * .5 * Math.pow(1 - p, 1.5);
          apertureFrames.push({ ...shape(x, y, w, h, radius), offset: .18 + .82 * t });
        }
        for (const element of root.querySelectorAll(".startup-aperture-shape")) {
          animate(element, apertureFrames, APERTURE_END - APERTURE_AT, APERTURE_AT, "linear");
          animate(element, [{ opacity: 0 }, { opacity: 1 }], 100, APERTURE_AT, "linear");
        }
        animate(root.querySelector(".startup-aperture-rim"), [
          { strokeWidth: fontSize * .132, offset: 0 },
          { strokeWidth: 2, offset: .65 },
          { strokeWidth: 0, offset: 1 },
        ], APERTURE_END - APERTURE_AT, APERTURE_AT, "linear");
        root.querySelectorAll(".startup-letter-clip").forEach((letter, index) => {
          if (index === 0) {
            animate(letter, [{ opacity: 1 }, { opacity: 0 }], 100, APERTURE_AT, "linear");
          } else {
            animate(letter, [
              { opacity: 1, transform: "none" },
              { opacity: 0, transform: `translate(${fontSize * (.15 + index * .015)}px,-8px) scale(.96)` },
            ], 210, 1840 + index * 12, "cubic-bezier(.4,0,.6,1)");
          }
        });
        animate(root.querySelector(".startup-tittle-squash"), [
          { opacity: 1 }, { opacity: 0 },
        ], 160, 1980, "linear");
        animate(logo, [
          { opacity: 1, transform: "none" }, { opacity: 0, transform: "translateX(-18px) scale(.94)" },
        ], 290, 1910);
        animate(skipRef.current, [{ opacity: 1 }, { opacity: 0 }], 180, 2780, "linear");
        timers.push(window.setTimeout(() => arrive(request.id), ARRIVAL_AT));
        timers.push(window.setTimeout(end, STARTUP_INTRO_DURATION));
      };
      // Font swapping after measuring would move the aperture. Wait for the
      // local Latin subset, and reveal the app if that asset cannot be loaded.
      const font = '600 18px "IBM Plex Sans"';
      if (document.fonts.check(font, "Obsession")) startFull();
      else {
        const deadline = window.setTimeout(() => finish(request.id, false), 500);
        timers.push(deadline);
        void document.fonts.load(font, "Obsession").then(() => {
          window.clearTimeout(deadline);
          startFull();
        }, () => { if (!disposed) finish(request.id, false); });
      }
    }

    return () => {
      disposed = true;
      timers.forEach(window.clearTimeout);
      animations.forEach((animation) => animation.cancel());
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("resize", end);
      if (!previouslyInert) content?.removeAttribute("inert");
      if (request.mode === "full") {
        const focus = previousFocus?.isConnected && previousFocus !== document.body
          ? previousFocus : shell?.querySelector<HTMLElement>(".nav-item[aria-current='page']");
        focus?.focus({ preventScroll: true });
      }
    };
  }, [request, motionOff, hidden, theme]);

  if (!request || motionOff || hidden) return null;
  const colors = THEME_COLORS[theme];
  const style = { "--intro-accent": colors.accent, "--intro-surface": colors.surface } as CSSProperties;
  return (
    <div ref={rootRef} key={request.id} className="startup-intro" data-startup-mode={request.mode}
      role={request.mode === "full" ? "dialog" : "status"} aria-modal={request.mode === "full" ? true : undefined}
      aria-label={request.mode === "full" ? "Добро пожаловать в Obsession" : "Obsession"}
      tabIndex={request.mode === "full" ? -1 : undefined} style={style}>
      {request.mode === "quick" ? <div className="startup-quick-veil" aria-hidden="true" /> : <>
        <svg className="startup-aperture" aria-hidden="true" preserveAspectRatio="none">
          <defs><mask id={apertureId} maskUnits="userSpaceOnUse" x="0" y="0" width="100%" height="100%">
            <rect width="100%" height="100%" fill="white" />
            <rect className="startup-aperture-shape" fill="black" />
          </mask></defs>
          <rect width="100%" height="100%" fill="var(--intro-surface)" mask={`url(#${apertureId})`} />
          <rect className="startup-aperture-shape startup-aperture-rim" fill="none" stroke="#f3efe5" />
        </svg>
        <div className="startup-lockup" aria-hidden="true">
          <div className="startup-logo">
            <div className="startup-logo-clip"><div className="startup-logo-art">
              {theme === "goldenmeadow" ? <GoldenMeadowCore size={88} active={false} interactive={false} paused />
                : <img src={brandArtwork} alt="" draggable={false} />}
            </div></div>
          </div>
          <div className="startup-wordmark font-display">
            {Array.from("Obsession", (letter, index) => <span className={`startup-letter-clip${letter === "i" ? " startup-letter-i" : ""}`} key={index}>
              <span className="startup-letter"><span className="startup-glyph">{letter}</span></span>
            </span>)}
            <span className="startup-tittle"><span className="startup-tittle-squash"><span className="startup-tittle-ink">i</span></span></span>
          </div>
          <div className="startup-caption">{THEMES.find((entry) => entry.id === theme)?.label}</div>
        </div>
        <button ref={skipRef} type="button" className="startup-skip no-drag" onClick={() => useStartupIntro.getState().finish(request.id)}>
          Пропустить <span>Esc</span>
        </button>
      </>}
    </div>
  );
}

export function StartupIntroReplayButton() {
  const motionOff = useMotionOff();
  return <button type="button" className="startup-replay no-drag" disabled={motionOff}
    onClick={() => useStartupIntro.getState().replay()}>
    Посмотреть вступление<span aria-hidden="true">↗</span>
  </button>;
}
