import { useState } from "react";
import ReactDOM from "react-dom/client";

import { MidnightSpriteLab } from "./design/components/MidnightSpriteLab";
import type { MidnightDetail } from "./design/components/MidnightSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/fonts.css";
import "./styles/midnightLab.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const CORE_SIZES = [240, 104] as const;

const COPY: Record<ObsessionVisualPhase, { title: string; text: string }> = {
  idle: {
    title: "Свет полуночного фонаря",
    text: "Кованый уличный фонарь льет тихий конус света в ночную морось. Под ним на каменной тумбе сладко дремлет котик в теплом шарфе.",
  },
  engaging: {
    title: "Мерцание ночного светильника",
    text: "Холодная лампа мягко разгорается серебряно-голубым сиянием, конус света наполняется светящейся моросью.",
  },
  scanning: {
    title: "Взгляд сквозь туман",
    text: "Кот приоткрывает светящиеся неоновые глаза, сканируя ночной переулок.",
  },
  focused: {
    title: "Полуночный страж",
    text: "Фонарь ярко сияет чистым льдисто-белым светом, глаза кота искрятся небесным неоном.",
  },
  fault: {
    title: "Вспышка во тьме",
    text: "Фонарь тревожно мигает, кот вздрагивает и настороженно прижимает ушки.",
  },
};

function MidnightLab() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(240);
  const [detail, setDetail] = useState<MidnightDetail>("auto");
  const [paused, setPaused] = useState(false);
  const active = phase === "focused";

  return (
    <main className="midnight-lab-shell">
      <section className="midnight-lab-stage">
        <header className="midnight-lab-controls" aria-label="Midnight lab controls">
          <label>
            state
            <select value={phase} onChange={(event) => setPhase(event.target.value as ObsessionVisualPhase)}>
              {PHASES.map((item) => <option key={item}>{item}</option>)}
            </select>
          </label>
          <label>
            core
            <select value={coreSize} onChange={(event) => setCoreSize(Number(event.target.value) as (typeof CORE_SIZES)[number])}>
              {CORE_SIZES.map((item) => <option key={item} value={item}>{item}px</option>)}
            </select>
          </label>
          <label>
            detail
            <select value={detail} onChange={(event) => setDetail(event.target.value as MidnightDetail)}>
              <option value="auto">auto</option>
              <option value="base">base</option>
              <option value="hero">hero</option>
            </select>
          </label>
          <button type="button" onClick={() => setPaused((value) => !value)}>
            {paused ? "Motion: paused" : "Motion: running"}
          </button>
        </header>

        <div className="midnight-lab-content">
          <section className="midnight-lab-hero">
            <div className="midnight-lab-copy">
              <span className="midnight-lab-kicker">MIDNIGHT / PIXEL CORE LAB</span>
              <h1>Уличный фонарь<br />и сонный кот</h1>
              <strong>{COPY[phase].title}</strong>
              <p>{COPY[phase].text}</p>
              <small>52×52 integer pixel grid · midnight indigo & ice-blue streetlight</small>
            </div>

            <div className="midnight-lab-core-well">
              <button
                aria-label={active ? "Усыпить луну" : "Разбудить полуночника"}
                className="midnight-lab-core-button"
                type="button"
                onClick={() => setPhase(active ? "idle" : "focused")}
                style={{ width: coreSize, height: coreSize }}
              >
                <MidnightSpriteLab detail={detail} phase={phase} paused={paused} size={coreSize} />
              </button>
              <span>{active ? "CLICK · SLEEP" : "CLICK · WAKE"}</span>
            </div>
          </section>

          <section className="midnight-lab-state-strip" aria-label="All animation states">
            {PHASES.map((item) => (
              <button
                className="midnight-lab-state-card"
                data-selected={item === phase || undefined}
                key={item}
                onClick={() => setPhase(item)}
                type="button"
              >
                <MidnightSpriteLab detail="base" phase={item} paused={paused} size={104} />
                <span>{item}</span>
              </button>
            ))}
          </section>
        </div>
      </section>
    </main>
  );
}

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(<MidnightLab />);

if (import.meta.hot) {
  import.meta.hot.dispose(() => root.unmount());
}
