import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";

import { ObsessionChoirCore } from "./design/components/ObsessionChoirCore";
import { ObsessionChoirFallback } from "./design/components/ObsessionChoirFallback";
import { ObsessionChoirField } from "./design/components/ObsessionChoirField";
import type { QualityTier } from "./design/frameScheduler";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/globals.css";
import "./styles/obsessionChoirDev.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const SCREENS = ["overview", "dpi", "telegram"] as const;
const QUALITY: readonly QualityTier[] = ["high", "balanced", "low"];
type Screen = (typeof SCREENS)[number];

function paramValue<T extends string>(name: string, allowed: readonly T[], fallback: T): T {
  const value = new URLSearchParams(window.location.search).get(name) as T | null;
  return value && allowed.includes(value) ? value : fallback;
}

function ChoirPanel({ className = "", children }: { className?: string; children: React.ReactNode }) {
  return <section data-choir-lens className={`choir-panel ${className}`}>{children}</section>;
}

function ScreenCopy({ screen }: { screen: Screen }) {
  if (screen === "dpi") {
    return {
      eyebrow: "DPI BYPASS",
      title: "Aperture control",
      description: "The seal conducts the field. The master eye belongs to the space behind the interface.",
    };
  }
  if (screen === "telegram") {
    return {
      eyebrow: "TELEGRAM RELAY",
      title: "Quiet transmission",
      description: "The choir stays submerged while the connection stabilizes inside optical black glass.",
    };
  }
  return {
    eyebrow: "SYSTEM OVERVIEW",
    title: "The Black Choir",
    description: "One eye is visible. Nine more exist only where the engraving briefly agrees with itself.",
  };
}

function Harness() {
  const fixedViewport = new URLSearchParams(window.location.search).get("viewport") === "800x600";
  const [phase, setPhase] = useState<ObsessionVisualPhase>(() => paramValue("phase", PHASES, "idle"));
  const [screen, setScreen] = useState<Screen>(() => paramValue("screen", SCREENS, "overview"));
  const [quality, setQuality] = useState<QualityTier>(() => paramValue("quality", QUALITY, "high"));
  const [paused, setPaused] = useState(() => new URLSearchParams(window.location.search).get("paused") === "1");
  const [fallback, setFallback] = useState(() => new URLSearchParams(window.location.search).get("fallback") === "1");
  const [ritual, setRitual] = useState(false);
  const ritualTimer = useRef<number | null>(null);
  const copy = useMemo(() => ScreenCopy({ screen }), [screen]);
  const active = phase === "focused";

  useEffect(() => () => {
    if (ritualTimer.current != null) window.clearTimeout(ritualTimer.current);
  }, []);

  const forceRitual = () => {
    if (ritualTimer.current != null) window.clearTimeout(ritualTimer.current);
    setRitual(true);
    ritualTimer.current = window.setTimeout(() => setRitual(false), 1450);
  };

  return (
    <div
      data-theme="obsession"
      data-choir-paused={paused ? "true" : undefined}
      data-choir-viewport={fixedViewport ? "800x600" : undefined}
      className="choir-dev-root"
    >
      <div className="choir-scene" aria-hidden="true">
        {fallback ? (
          <ObsessionChoirFallback phase={phase} screen={screen} paused={paused} forceRitual={ritual} />
        ) : (
          <ObsessionChoirField
            key={quality}
            phase={phase}
            screen={screen}
            paused={paused}
            forceRitual={ritual}
            qualityTier={quality}
          />
        )}
      </div>

      <header className="choir-titlebar">
        <div className="choir-wordmark"><i /> Obsession <span>THE BLACK CHOIR</span></div>
        <div className="choir-window-dots"><i /><i /><i /></div>
      </header>

      <aside className="choir-rail">
        <div className="choir-brand">
          <div className="choir-brand-seal"><span /></div>
          <div><strong>Obsession</strong><small>V1.1.0 · PROTOTYPE</small></div>
        </div>
        <div className="choir-nav-label">MENU</div>
        <nav>
          {SCREENS.map((item, index) => (
            <button key={item} type="button" className={screen === item ? "active" : ""} onClick={() => setScreen(item)}>
              <span>{["◈", "ϟ", "⌁"][index]}</span>{item === "dpi" ? "DPI bypass" : item}
            </button>
          ))}
          <button type="button"><span>⌬</span>Strategy</button>
          <button type="button"><span>≡</span>Lists</button>
          <button type="button"><span>◇</span>Profiles</button>
        </nav>
        <small className="choir-signature">made by AcediaWhy</small>
      </aside>

      <main className="choir-stage">
        <div className="choir-heading">
          <div><span>{copy.eyebrow}</span><h1>{copy.title}</h1><p>{copy.description}</p></div>
          <div className={`choir-status ${phase}`}><i />{phase}</div>
        </div>

        <div className="choir-content-grid">
          <ChoirPanel className="choir-control-panel">
            <div className="choir-core-wrap">
              <ObsessionChoirCore
                active={active}
                busy={phase === "engaging"}
                scanning={phase === "scanning"}
                alarm={phase === "fault"}
                paused={paused}
                forceRitual={ritual}
                onClick={() => setPhase(active ? "idle" : "focused")}
                size={232}
              />
              <strong>{active ? "APERTURE HELD" : "ENGAGE THE SEAL"}</strong>
              <span>The seal is a conductor, not another eye.</span>
            </div>
            <div className="choir-separator" />
            <div className="choir-setting-row"><span>OPTICAL ENGINE</span><strong>Black Choir · V1</strong></div>
            <div className="choir-setting-row"><span>ALIGNMENT</span><strong>{phase === "focused" ? "Coherent" : "Submerged"}</strong></div>
          </ChoirPanel>

          <div className="choir-side-stack">
            <ChoirPanel className="choir-log-panel">
              <div className="choir-panel-head"><span>FIELD NOTES</span><button type="button">CLEAR</button></div>
              <div className="choir-log-lines">
                <p><time>00:14:22</time><span>Master contour acquired in negative space.</span></p>
                <p><time>00:14:24</time><span>Nine latent vectors remain below recognition.</span></p>
                <p><time>00:14:27</time><span>Panel refraction registered · {quality} tier.</span></p>
              </div>
              <div className="choir-log-caret" />
            </ChoirPanel>
            <ChoirPanel className="choir-metric-panel">
              <div><span>COHERENCE</span><strong>{phase === "fault" ? "BROKEN" : phase === "focused" ? "99.8%" : "12.4%"}</strong></div>
              <div><span>VISIBLE EYES</span><strong>{ritual ? "10" : "1"}</strong></div>
              <div><span>DEPTH</span><strong>III</strong></div>
            </ChoirPanel>
          </div>
        </div>
      </main>

      <div className="choir-dev-controls">
        <div className="choir-control-group">
          <span>PHASE</span>
          {PHASES.map((item) => <button key={item} type="button" className={phase === item ? "active" : ""} onClick={() => setPhase(item)}>{item}</button>)}
        </div>
        <div className="choir-control-group">
          <span>QUALITY</span>
          {QUALITY.map((item) => <button key={item} type="button" className={quality === item ? "active" : ""} onClick={() => setQuality(item)}>{item}</button>)}
        </div>
        <button type="button" className={ritual ? "active" : ""} onClick={forceRitual}>ritual</button>
        <button type="button" className={fallback ? "active" : ""} onClick={() => setFallback((value) => !value)}>fallback</button>
        <button type="button" className={paused ? "active" : ""} onClick={() => setPaused((value) => !value)}>{paused ? "resume" : "pause"}</button>
      </div>
    </div>
  );
}

const rootHost = document.getElementById("root")!;
const choirGlobal = globalThis as typeof globalThis & { __obsessionChoirRoot?: Root };
const root = choirGlobal.__obsessionChoirRoot ?? createRoot(rootHost);
choirGlobal.__obsessionChoirRoot = root;
root.render(<Harness />);
