import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";

import { Icon } from "./design/components/icons";
import { YaniCharacterScene } from "./design/components/YaniCharacterScene";
import { YaniEarScene } from "./design/components/YaniEarScene";
import type { EarMood, EarQuality } from "./design/components/yaniEar/types";
import "./styles/fonts.css";
import "./styles/earLab.css";

const MOODS: readonly EarMood[] = ["idle", "busy", "scanning", "active", "alarm"];
const QUALITIES: readonly EarQuality[] = ["high", "balanced", "low"];
const DEBUG_CAPTURE = new URLSearchParams(window.location.search).has("capture");
const NAV_ITEMS = [
  { id: "overview", label: "Обзор", icon: Icon.Shield },
  { id: "dpi", label: "DPI-обход", icon: Icon.Bolt },
  { id: "ai", label: "ИИ-разблокировка", icon: Icon.Robot },
  { id: "telegram", label: "Telegram", icon: Icon.Send },
  { id: "lists", label: "Списки", icon: Icon.List },
  { id: "profiles", label: "Профили", icon: Icon.Layers },
  { id: "settings", label: "Настройки", icon: Icon.Settings },
] as const;

function EarLab() {
  const [mood, setMood] = useState<EarMood>("idle");
  const [quality, setQuality] = useState<EarQuality>("high");
  const [paused, setPaused] = useState(false);
  const [compact, setCompact] = useState(false);
  const [showUi, setShowUi] = useState(true);
  const [activeNav, setActiveNav] = useState<(typeof NAV_ITEMS)[number]["id"]>("overview");

  useEffect(() => {
    document.documentElement.dataset.earMood = mood;
    return () => { delete document.documentElement.dataset.earMood; };
  }, [mood]);

  return (
    <div className="ear-lab-shell">
      <section className="ear-lab-stage" data-viewport={compact ? "800x600" : "1000x680"} data-mood={mood}>
        <div className="ear-lab-ambient" />
        <div className="ear-lab-backdrop-details" aria-hidden="true">
          <span className="ear-lab-room-grid" />
          <span className="ear-lab-blind-shadow" />
          <span className="ear-lab-cable-loop" />
          <span className="ear-lab-mint-panel ear-lab-mint-panel--a" />
          <span className="ear-lab-mint-panel ear-lab-mint-panel--b" />
          <span className="ear-lab-mint-panel ear-lab-mint-panel--c" />
          <span className="ear-lab-mint-orbit ear-lab-mint-orbit--a" />
          <span className="ear-lab-mint-orbit ear-lab-mint-orbit--b" />
          <span className="ear-lab-mint-particles" />
          <div className="ear-lab-receipt">
            <span>YANI MART / NO.07</span>
            <strong>170円</strong>
            <small>6 DAYS · 1 CIG</small>
            <i>03:17</i>
          </div>
          <div className="ear-lab-wall-stamp">
            <span>ROOM SIGNAL</span>
            <strong>03:17</strong>
            <small>STILL AWAKE</small>
          </div>
          <div className="ear-lab-sleep-note">
            <svg className="ear-lab-cat-doodle" viewBox="0 0 110 58" aria-hidden="true">
              <path d="M18 34c0-9 5-17 13-22l8 9c5-2 10-3 16-3s11 1 16 3l8-9c8 5 13 13 13 22 0 13-14 20-37 20S18 47 18 34Z" />
              <path d="M33 34c4 4 9 4 13 0M64 34c4 4 9 4 13 0M52 42l3 2 3-2M55 44v4" />
              <path d="M27 41 10 39M28 45 12 48M83 41l17-2M82 45l16 3" />
            </svg>
            <strong lang="ja">ねむい…</strong>
          </div>
          <svg className="ear-lab-sleep-smoke" viewBox="0 0 78 96" aria-hidden="true">
            <path pathLength="1" d="M48 94c-13-13 10-20-2-34-10-12-2-20 7-28 8-7 5-19-1-29" />
            <path pathLength="1" d="M29 90c9-10-5-17 2-27 7-9 2-16-3-22-6-8-1-16 4-22" />
          </svg>
          <span className="ear-lab-cup-ring" />
          <span className="ear-lab-shelf-line" />
        </div>
        <YaniCharacterScene
          mood={mood}
          quality={quality}
          paused={paused}
          debugCapture={DEBUG_CAPTURE}
          className="ear-lab-field"
        />
        <div className="ear-lab-grain" />
        <div className="ear-lab-flying-cigarette" aria-hidden="true">
          <span className="ear-lab-cigarette-smoke" />
          <span className="ear-lab-cigarette-stick" />
        </div>

        {showUi && (
          <div className="ear-lab-ui">
            <header>
              <span className="ear-lab-brand-dot" />
              <strong>Obsession</strong>
              <span>YANI / MATERIAL STUDY</span>
            </header>
            <aside
              className="ear-lab-nav-shell"
              data-burning={mood === "busy" || mood === "active" || undefined}
              aria-label="Прототип боковой навигации Yani Neko"
            >
              <div className="ear-lab-cabbage-leaves" aria-hidden="true">
                <i />
                <i />
                <i />
                <i />
                <i />
                <i />
                <i />
              </div>
              <div className="ear-lab-nav-cap" aria-hidden="true">
                <span>YANI</span>
                <i />
                <span>03:17</span>
              </div>
              <div className="ear-lab-nav-foil" aria-hidden="true" />
              <nav className="ear-lab-nav-list" aria-label="Разделы">
                {NAV_ITEMS.map((item, index) => {
                  const active = activeNav === item.id;
                  const NavIcon = item.icon;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      className={active ? "is-active" : undefined}
                      aria-current={active ? "page" : undefined}
                      onClick={() => setActiveNav(item.id)}
                    >
                      <span className="ear-lab-nav-number" aria-hidden="true">{String(index + 1).padStart(2, "0")}</span>
                      <span className="ear-lab-nav-icon" aria-hidden="true"><NavIcon size={15} /></span>
                      <span className="ear-lab-nav-label">{item.label}</span>
                      <span className="ear-lab-nav-ember" aria-hidden="true" />
                    </button>
                  );
                })}
              </nav>
              <div className="ear-lab-nav-foot" aria-hidden="true">
                <span>STILL AWAKE</span>
                <span>NO. 07</span>
              </div>
            </aside>
            <div className="ear-lab-glass ear-lab-glass--hero">
              <small>03:17 / STILL AWAKE</small>
              <h1>Живая модель вместо постера</h1>
              <p>Yani дышит, двигает ушами и хвостом, а состояние приложения меняет характер движения.</p>
            </div>
            <div className="ear-lab-glass ear-lab-glass--status">
              <small>STATE</small>
              <strong>{mood}</strong>
            </div>
          </div>
        )}

        <aside className="ear-lab-core-card">
          <span>CORE CROP</span>
          <div className="ear-lab-core-viewport">
            <YaniEarScene
              mood={mood}
              quality={quality}
              paused={paused}
              variant="core"
              debugCapture={DEBUG_CAPTURE}
              className="ear-lab-core"
            />
          </div>
          <small>только уши · без лица</small>
        </aside>

        <div className="ear-lab-controls" aria-label="Ear laboratory controls">
          <label>state
            <select value={mood} onChange={(event) => setMood(event.target.value as EarMood)}>
              {MOODS.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <label>quality
            <select value={quality} onChange={(event) => setQuality(event.target.value as EarQuality)}>
              {QUALITIES.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <button type="button" data-on={paused || undefined} onClick={() => setPaused((value) => !value)}>pause</button>
          <button type="button" data-on={compact || undefined} onClick={() => setCompact((value) => !value)}>{compact ? "1000×680" : "800×600"}</button>
          <button type="button" data-on={!showUi || undefined} onClick={() => setShowUi((value) => !value)}>{showUi ? "hide UI" : "show UI"}</button>
        </div>

        <footer>Move the pointer across either ear. The nearest pinna leads; the second follows.</footer>
      </section>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode><EarLab /></React.StrictMode>,
);
