import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";

import { NavRail, type Tab } from "./design/components/NavRail";
import { YaniCatSpriteLab } from "./design/components/YaniCatSpriteLab";
import { YaniCharacterField } from "./design/components/YaniCharacterField";
import type { YaniStoryDensity, YaniStoryDepth } from "./design/components/YaniStoryDetails";
import { YaniTamagotchiLab } from "./design/components/YaniTamagotchiLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import type { QualityTier } from "./design/render";
import { useThemeStore } from "./store/themeStore";
import "./styles/fonts.css";
import "./styles/globals.css";
import "./styles/yaniDev.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const QUALITIES: readonly QualityTier[] = ["high", "balanced", "low"];
const CORE_SIZES = [240, 220, 104] as const;
const STORY_DENSITIES: readonly Exclude<YaniStoryDensity, "off">[] = ["quiet", "story", "chaotic"];
const STORY_DEPTHS: readonly YaniStoryDepth[] = ["flat", "layered", "deep"];
type CoreVariant = "cat-only" | "cat-device" | "legacy";

function YaniDevHarness() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [quality, setQuality] = useState<QualityTier>("high");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(240);
  const [coreVariant, setCoreVariant] = useState<CoreVariant>("cat-only");
  const [storyDensity, setStoryDensity] = useState<Exclude<YaniStoryDensity, "off">>("story");
  const [storyDepth, setStoryDepth] = useState<YaniStoryDepth>("deep");
  const [screen, setScreen] = useState<Tab>("overview");
  const [forceFallback, setForceFallback] = useState(false);
  const [paused, setPaused] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [compact, setCompact] = useState(false);
  const [crossfading, setCrossfading] = useState(false);
  const crossfadeTimer = useRef<number>();
  const stageRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const previousTheme = useThemeStore.getState().theme;
    useThemeStore.setState({ theme: "yanineko" });
    document.documentElement.dataset.theme = "yanineko";
    return () => {
      useThemeStore.setState({ theme: previousTheme });
      delete document.documentElement.dataset.reduceMotion;
      if (crossfadeTimer.current) window.clearTimeout(crossfadeTimer.current);
    };
  }, []);

  useEffect(() => {
    if (reducedMotion) document.documentElement.dataset.reduceMotion = "true";
    else delete document.documentElement.dataset.reduceMotion;
  }, [reducedMotion]);

  const effectivePaused = paused || reducedMotion;
  const active = phase === "focused";
  const busy = phase === "engaging";
  const scanning = phase === "scanning";
  const alarm = phase === "fault";
  const standaloneCore = coreVariant === "cat-only";

  const simulateContextLoss = () => {
    const canvas = document.querySelector<HTMLCanvasElement>("canvas[data-yani-character-scene]");
    canvas?.getContext("webgl2")?.getExtension("WEBGL_lose_context")?.loseContext();
  };

  const simulateCrossfade = () => {
    setCrossfading(true);
    if (crossfadeTimer.current) window.clearTimeout(crossfadeTimer.current);
    crossfadeTimer.current = window.setTimeout(() => setCrossfading(false), 420);
  };

  const updateParallax = (event: React.PointerEvent<HTMLDivElement>) => {
    const stage = stageRef.current;
    if (!stage || storyDepth === "flat" || reducedMotion) return;
    const rect = stage.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width - 0.5) * 2;
    const y = ((event.clientY - rect.top) / rect.height - 0.5) * 2;
    const strength = storyDepth === "deep" ? 1 : 0.58;
    stage.style.setProperty("--yani-near-x", `${x * 9 * strength}px`);
    stage.style.setProperty("--yani-near-y", `${y * 7 * strength}px`);
    stage.style.setProperty("--yani-mid-x", `${x * 4 * strength}px`);
    stage.style.setProperty("--yani-mid-y", `${y * 3 * strength}px`);
  };

  const resetParallax = () => {
    const stage = stageRef.current;
    stage?.style.setProperty("--yani-near-x", "0px");
    stage?.style.setProperty("--yani-near-y", "0px");
    stage?.style.setProperty("--yani-mid-x", "0px");
    stage?.style.setProperty("--yani-mid-y", "0px");
  };

  return (
    <div className="yani-dev-shell">
      <div
        ref={stageRef}
        className="yani-dev-stage"
        data-viewport={compact ? "800x600" : "1000x680"}
        data-crossfading={crossfading || undefined}
        data-story-density={storyDensity}
        data-story-depth={storyDepth}
        onPointerMove={updateParallax}
        onPointerLeave={resetParallax}
      >
        <div className="yani-dev-field-layer">
          <YaniCharacterField
            phase={phase}
            screen={screen}
            paused={effectivePaused}
            qualityTier={quality}
            forceFallback={forceFallback}
            storyDensity={storyDensity}
            storyDepth={storyDepth}
          />
        </div>

        <div className="yani-dev-app-layer">
          <NavRail active={screen} onSelect={setScreen} />
          <main className="yani-dev-main">
            <section className="glass spotlight yani-dev-hero-panel">
              <div>
                <div className="yani-dev-kicker">room 03:17 / {quality} / {forceFallback ? "canvas" : "webgl2"}</div>
                <h1 className="text-gradient">{standaloneCore ? "Yani Cat" : "Yani Pet"}</h1>
                <p>{standaloneCore ? "Пиксельная кошка становится самостоятельным живым ядром темы." : "Карманный тамагочи хранит состояние Obsession внутри кошачьей пиксельной Yani."}</p>
              </div>
              <div className="yani-dev-core-wrap" data-core-size={coreSize}>
                <div
                  className="yani-dev-core-prototype"
                  data-core-variant={coreVariant}
                  style={{ width: coreSize, height: coreSize }}
                >
                  {coreVariant === "cat-only" ? (
                    <button
                      aria-label="Toggle standalone Yani cat core"
                      className="yani-dev-cat-core-button"
                      type="button"
                      onClick={() => setPhase(active ? "idle" : "focused")}
                    >
                      <YaniCatSpriteLab phase={phase} paused={effectivePaused} standalone />
                    </button>
                  ) : (
                    <>
                      <YaniTamagotchiLab
                        active={active}
                        busy={busy}
                        scanning={scanning}
                        alarm={alarm}
                        paused={effectivePaused}
                        size={coreSize}
                        onClick={() => setPhase(active ? "idle" : "focused")}
                      />
                      {coreVariant === "cat-device" && <YaniCatSpriteLab phase={phase} paused={effectivePaused} />}
                    </>
                  )}
                </div>
              </div>
              <span className="spotlight-ring" />
            </section>

            <div className="yani-dev-lower-grid">
              <section className="glass spotlight yani-dev-card">
                <span className="yani-dev-card-label">Состояние</span>
                <strong>{phase}</strong>
                <p>{alarm ? "Уши опущены, силуэт коротко вздрагивает." : scanning ? "Yani проснулась и внимательно смотрит по сторонам." : busy ? "Пиксельные лапки торопливо перебирают шаги." : active ? "Появляется маленькое сердце, хвост довольно двигается." : "Yani спокойно дремлет, а над ухом иногда появляется тихое zzz."}</p>
                <span className="spotlight-ring" />
              </section>
              <section className="glass spotlight yani-dev-card yani-dev-card--quiet">
                <span className="yani-dev-card-label">Комната</span>
                <strong>{standaloneCore ? "YANI CAT / NO.07" : "YANI PET / NO.07"}</strong>
                <p>{standaloneCore ? "Кошачий силуэт показывает состояние защиты без дополнительного корпуса или предмета вокруг него." : "Личная вещь Yani показывает состояние защиты, не превращая комнату в витрину сетевого оборудования."}</p>
                <span className="spotlight-ring" />
              </section>
            </div>
          </main>
        </div>

        <aside className="yani-dev-controls" aria-label="Yani Neko visual controls">
          <label>state
            <select value={phase} onChange={(event) => setPhase(event.target.value as ObsessionVisualPhase)}>
              {PHASES.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <label>quality
            <select value={quality} onChange={(event) => setQuality(event.target.value as QualityTier)}>
              {QUALITIES.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <label>core
            <select value={coreSize} onChange={(event) => setCoreSize(Number(event.target.value) as (typeof CORE_SIZES)[number])}>
              {CORE_SIZES.map((item) => <option key={item} value={item}>{item}px</option>)}
            </select>
          </label>
          <label>core art
            <select value={coreVariant} onChange={(event) => setCoreVariant(event.target.value as CoreVariant)}>
              <option value="cat-only">cat only</option>
              <option value="cat-device">cat + pet</option>
              <option value="legacy">old pet</option>
            </select>
          </label>
          <label>props
            <select value={storyDensity} onChange={(event) => setStoryDensity(event.target.value as Exclude<YaniStoryDensity, "off">)}>
              {STORY_DENSITIES.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <label>depth
            <select value={storyDepth} onChange={(event) => setStoryDepth(event.target.value as YaniStoryDepth)}>
              {STORY_DEPTHS.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <button type="button" data-on={forceFallback || undefined} onClick={() => setForceFallback((value) => !value)}>Canvas fallback</button>
          <button type="button" data-on={paused || undefined} onClick={() => setPaused((value) => !value)}>Motion pause</button>
          <button type="button" data-on={reducedMotion || undefined} onClick={() => setReducedMotion((value) => !value)}>Reduced motion</button>
          <button type="button" data-on={compact || undefined} onClick={() => setCompact((value) => !value)}>{compact ? "1000×680" : "800×600"}</button>
          <button type="button" onClick={simulateContextLoss} disabled={forceFallback}>Lose context</button>
          <button type="button" onClick={simulateCrossfade}>Crossfade</button>
        </aside>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode><YaniDevHarness /></React.StrictMode>,
);
