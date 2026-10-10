import { createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { Theme } from "../../store/themeStore";
import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { useRenderActive } from "../render";
import { createAtmosphereController, type AtmosphereEvent } from "../themeAtmosphere";
import "../../styles/themeAtmosphere.css";

type SceneEvent = AtmosphereEvent & { theme: Theme; screen: string; target: string | null; x: number; y: number };
type AtmosphereContextValue = {
  event: SceneEvent | null;
  register(id: string, element: HTMLElement): () => void;
  preview(): void;
};
const AtmosphereContext = createContext<AtmosphereContextValue | null>(null);
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export function ThemeAtmosphere({ theme, phase, screen = "overview", children }: { theme: Theme; phase: ObsessionVisualPhase; screen?: string; children: ReactNode }) {
  const renderOn = useRenderActive();
  const [event, setEvent] = useState<SceneEvent | null>(null);
  const panels = useRef(new Map<string, HTMLElement>());
  const controller = useRef<ReturnType<typeof createAtmosphereController> | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const register = useCallback((id: string, element: HTMLElement) => {
    panels.current.set(id, element);
    return () => { panels.current.delete(id); };
  }, []);
  const preview = useCallback(() => controller.current?.preview(), []);

  useEffect(() => {
    setEvent(null);
    const next = createAtmosphereController({
      setTimeout: (callback, delay) => window.setTimeout(callback, delay),
      clearTimeout: (id) => window.clearTimeout(id as number),
      random: Math.random,
    }, (gesture) => {
      if (!gesture) { setEvent(null); return; }
      const visible = [...panels.current].filter(([, element]) => {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0 && rect.bottom > 40 && rect.top < window.innerHeight && rect.right > 0 && rect.left < window.innerWidth;
      });
      const notes = visible.filter(([, element]) => element.classList.contains("overview-service"));
      const characters = visible.filter(([, element]) => element.classList.contains("atmosphere-scene-source") && element.dataset.atmosphereTheme === theme);
      const surfaces = visible.filter(([, element]) => !element.classList.contains("atmosphere-scene-source"));
      const sleepy = gesture.kind === "rare" && (theme === "catnap" || theme === "yanineko");
      const candidates = sleepy && characters.length ? characters : notes.length ? notes : surfaces;
      const selected = candidates[Math.min(candidates.length - 1, Math.floor(gesture.variation * candidates.length))];
      const rect = selected?.[1].getBoundingClientRect();
      setEvent({ ...gesture, theme, screen, target: selected?.[0] ?? null,
        x: rect ? clamp(rect.right - 20, 0, window.innerWidth) / window.innerWidth : .78,
        y: rect ? clamp(rect.bottom, 40, window.innerHeight) / window.innerHeight : .7,
      });
    }, phaseRef.current);
    controller.current = next;
    next.setEnabled(renderOn);
    return () => { next.dispose(); controller.current = null; };
  }, [theme, screen, renderOn]);

  useEffect(() => { controller.current?.setPhase(phase); }, [phase]);
  // An old event never flashes in a newly selected theme or a still frame.
  const current = renderOn && event?.theme === theme && event.screen === screen ? event : null;
  const value = useMemo(() => ({ event: current, register, preview }), [current, register, preview]);
  return <AtmosphereContext.Provider value={value}>{children}</AtmosphereContext.Provider>;
}

function eventStyle(event: SceneEvent): CSSProperties {
  return {
    "--atmosphere-duration": `${event.duration}ms`,
    "--atmosphere-x": `${event.x * 100}%`,
    "--atmosphere-y": `${event.y * 100}%`,
  } as CSSProperties;
}

export function ThemeAtmosphereBackdrop() {
  const event = useContext(AtmosphereContext)?.event;
  if (!event) return null;
  return <div key={event.id} className="atmosphere-world" data-atmosphere-kind={event.kind} data-atmosphere-theme={event.theme} aria-hidden="true" style={eventStyle(event)}>
    <div className="atmosphere-world-light" />
    <div className="atmosphere-world-ray" />
    {event.kind === "rare" && event.theme === "midnight" && <div className="atmosphere-headlights-scene"><i /><i /></div>}
    {event.kind === "rare" && event.theme === "aurora" && <svg className="atmosphere-constellation" viewBox="0 0 160 110" fill="none">
      <path className="atmosphere-constellation-line" pathLength="1" d="m14 72 30-43 34 19 29-31 37 44-35 29-31-42-22 42-42-18Z" />
      {[[14,72],[44,29],[78,48],[107,17],[144,61],[109,90],[56,90]].map(([x,y], index) => <g key={index} style={{ "--star-order": index } as CSSProperties}><circle cx={x} cy={y} r="7" opacity=".17" /><circle cx={x} cy={y} r="2" /></g>)}
    </svg>}
  </div>;
}

export function ThemeAtmosphereSurface() {
  const context = useContext(AtmosphereContext);
  const id = useId();
  const ref = useRef<HTMLSpanElement>(null);
  const [geometry, setGeometry] = useState<CSSProperties>({});
  const register = context?.register;
  const event = context?.event;
  useLayoutEffect(() => {
    const panel = ref.current?.parentElement;
    if (panel && register) return register(id, panel);
  }, [id, register]);
  useLayoutEffect(() => {
    const panel = ref.current?.parentElement;
    if (!event || !panel) return;
    const rect = panel.getBoundingClientRect();
    const x = event.x * window.innerWidth;
    const y = event.y * window.innerHeight;
    const distance = Math.hypot(x - rect.left - rect.width / 2, y - rect.top - rect.height / 2);
    setGeometry({
      "--atmosphere-spot-x": `${clamp((x - rect.left) / Math.max(1, rect.width) * 100, 8, 92)}%`,
      "--atmosphere-spot-y": `${clamp((y - rect.top) / Math.max(1, rect.height) * 100, 8, 92)}%`,
      "--atmosphere-reflection-delay": `${Math.min(560, distance * .7)}ms`,
      "--atmosphere-reflection-duration": `${event.duration - 800}ms`,
    } as CSSProperties);
  }, [event]);
  if (!context) return null;
  return <span ref={ref} className="atmosphere-surface" aria-hidden="true">
    {event && <span key={event.id} className="atmosphere-surface-event" data-atmosphere-kind={event.kind} style={{ ...eventStyle(event), ...geometry }}>
      <span className="atmosphere-reflection"><i className="atmosphere-reflection-edge" /><i className="atmosphere-reflection-sheen" /></span>
      {event.kind === "rare" && event.target === id && <AtmosphereVisitor theme={event.theme} />}
    </span>}
  </span>;
}

function AtmosphereVisitor({ theme }: { theme: Theme }) {
  if (theme === "goldenmeadow") return <span className="atmosphere-flight-anchor"><svg className="atmosphere-visitor atmosphere-leaf" viewBox="0 0 32 40" fill="none">
    <g className="atmosphere-leaf-shape"><path d="m16 2 4 9 6-3-1 9 6 2-9 7-5 2-1 8-2-1 1-8-9-3-5-7 8 1-2-9 7 4Z" fill="#dda34b" stroke="#79552f" strokeWidth=".9" />
    <path d="m16 9-1 22m0-10-6-4m6 7 8-7" stroke="#805b32" /></g>
  </svg></span>;
  if (theme === "ophanim") return <span className="atmosphere-flight-anchor"><svg className="atmosphere-visitor atmosphere-moth" viewBox="0 0 48 32" shapeRendering="crispEdges">
    <g className="atmosphere-moth-wings" fill="#f0d7a8" stroke="#69533f" strokeWidth="1.4"><path d="M22 13 16 6 3 2v13l5 4 9 5 5-7Z" /><path d="m26 13 6-7 13-4v13l-5 4-9 5-5-7Z" /></g>
    <path d="M22 9h4v16h-4Zm-3-5h3v6h-3Zm7 0h3v6h-3Z" fill="#5b4435" /><path d="M23 12h2v9h-2Z" fill="#f3dfac" />
  </svg></span>;
  if (theme === "japan") return <span className="atmosphere-rain-visitor"><i className="atmosphere-drop-trail" /><i className="atmosphere-drop-ripple" /><svg className="atmosphere-visitor atmosphere-drop" viewBox="0 0 20 30" fill="none"><path d="M10 2C8 8 2 15 2 20a8 8 0 0 0 16 0c0-5-6-12-8-18Z" fill="#c0e6ef" fillOpacity=".7" stroke="#e0f7fc" strokeOpacity=".9" /><path d="M6 17q-3 5 1 7" stroke="#f0fbff" strokeOpacity=".9" /></svg></span>;
  if (theme === "fallendown") return <svg className="atmosphere-visitor atmosphere-firefly" viewBox="0 0 32 32" shapeRendering="crispEdges"><g className="atmosphere-firefly-spark"><path d="M12 2h8v8h10v10H20v10h-8V20H2V10h10Z" fill="#efd586" opacity=".25" /><path d="M14 6h4v8h8v4h-8v8h-4v-8H6v-4h8Z" fill="#efd586" /><path d="M14 14h4v4h-4Z" fill="#fff3cc" /></g></svg>;
  if (theme === "catnap") return <svg className="atmosphere-visitor atmosphere-dream" viewBox="0 0 48 54" fill="none" stroke="#ffe2b4" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 39h8l-8 8h8" /><path d="M19 24h10L19 34h10" /><path d="M32 7h12L32 19h12" /></svg>;
  if (theme === "yanineko") return <svg className="atmosphere-visitor atmosphere-purr" viewBox="0 0 64 70" fill="#4f8061">
    {[[7,48,.65],[25,29,.85],[44,8,1]].map(([x,y,scale], index) => <g key={index} className="atmosphere-purr-mark"><g transform={`translate(${x} ${y}) scale(${scale})`}><ellipse cx="4" cy="7" rx="2.5" ry="3.4" /><ellipse cx="10" cy="3.5" rx="2.6" ry="3.5" /><ellipse cx="16" cy="7" rx="2.5" ry="3.4" /><path d="M10 10c-3 0-3 3-5 5-3 4 1 6 5 5 4 1 8-1 5-5-2-2-2-5-5-5Z" /></g></g>)}
  </svg>;
  return null;
}

export function ThemeAtmosphereSceneVisitor({ theme }: { theme: Theme }) {
  const context = useContext(AtmosphereContext);
  const id = useId();
  const ref = useRef<HTMLSpanElement>(null);
  const register = context?.register;
  const event = context?.event;
  useLayoutEffect(() => {
    if (ref.current && register) return register(id, ref.current);
  }, [id, register, theme]);
  const resting = event?.kind === "rare" && event.target === id;
  if (theme !== "catnap" && theme !== "yanineko") return null;
  return <span ref={ref} className="atmosphere-scene-source" data-atmosphere-theme={theme} aria-hidden="true">
    {resting && event.theme === theme && <span key={event.id} className="atmosphere-scene-event" style={eventStyle(event)}><AtmosphereVisitor theme={theme} /></span>}
  </span>;
}

// Development-only control, mounted by overviewDev rather than the application.
export function AtmospherePreviewButton() {
  const context = useContext(AtmosphereContext);
  const enabled = useRenderActive();
  return <button type="button" disabled={!enabled} onClick={context?.preview} className="rounded border border-white/20 px-2 py-1 disabled:opacity-40">Событие темы</button>;
}
