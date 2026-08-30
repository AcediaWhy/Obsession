import { useState } from "react";
import ReactDOM from "react-dom/client";

import { OphanimCatSpriteLab } from "./design/components/OphanimCatSpriteLab";
import type { OphanimCatDetail } from "./design/components/OphanimCatSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/fonts.css";
import "./styles/ophanimCatLab.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const CORE_SIZES = [260, 104] as const;

const COPY: Record<ObsessionVisualPhase, { title: string; text: string }> = {
  idle: {
    title: "Тихое бдение",
    text: "Кот спит, свернувшись в ступице. Обычные глаза закрыты, а золотые колёса едва дышат вокруг него.",
  },
  engaging: {
    title: "Колёса пробуждаются",
    text: "Уши приподнимаются, глаза на ободах открываются по очереди, и трон начинает набирать ход.",
  },
  scanning: {
    title: "Взор ищет",
    text: "Великое Око раскрывается на лбу. Луч и усы-антенны прочёсывают пространство, не меняя сонного лица кота.",
  },
  focused: {
    title: "Страж престола",
    text: "Золото переходит в магенту, кот приподнимается над ступицей, а все кольца смотрят в одну точку.",
  },
  fault: {
    title: "Красный обод",
    text: "Геометрия сбита: кольца вспыхивают красным, тело дрожит, но Великое Око остаётся открытым.",
  },
};

function OphanimCatLab() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(260);
  const [detail, setDetail] = useState<OphanimCatDetail>("auto");
  const [paused, setPaused] = useState(false);
  const active = phase === "focused";

  return (
    <main className="ophanim-cat-lab-shell">
      <section className="ophanim-cat-lab-stage">
        <header className="ophanim-cat-lab-controls" aria-label="Ophanim cat lab controls">
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
            <select value={detail} onChange={(event) => setDetail(event.target.value as OphanimCatDetail)}>
              <option value="auto">auto</option>
              <option value="base">base</option>
              <option value="hero">hero</option>
            </select>
          </label>
          <button type="button" onClick={() => setPaused((value) => !value)}>
            {paused ? "Motion: paused" : "Motion: running"}
          </button>
        </header>

        <div className="ophanim-cat-lab-content">
          <section className="ophanim-cat-lab-hero">
            <div className="ophanim-cat-lab-copy">
              <span className="ophanim-cat-lab-kicker">OPHANIM / FELINE THRONE LAB</span>
              <h1>Кот-ступица<br />и Великое Око</h1>
              <strong>{COPY[phase].title}</strong>
              <p>{COPY[phase].text}</p>
              <small>52×52 integer pixel grid · tail ring · third eye</small>
            </div>

            <div className="ophanim-cat-lab-core-well">
              <button
                aria-label={active ? "Усыпить небесного кота" : "Пробудить небесного кота"}
                className="ophanim-cat-lab-core-button"
                type="button"
                onClick={() => setPhase(active ? "idle" : "focused")}
                style={{ width: coreSize, height: coreSize }}
              >
                <OphanimCatSpriteLab detail={detail} phase={phase} paused={paused} size={coreSize} />
              </button>
              <span>{active ? "THRONE · AWAKE" : "CLICK · AWAKEN"}</span>
            </div>
          </section>

          <section className="ophanim-cat-lab-state-strip" aria-label="All animation states">
            {PHASES.map((item) => (
              <button
                className="ophanim-cat-lab-state-card"
                data-selected={item === phase || undefined}
                key={item}
                onClick={() => setPhase(item)}
                type="button"
              >
                <OphanimCatSpriteLab detail="base" phase={item} paused={paused} size={104} />
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
root.render(<OphanimCatLab />);

if (import.meta.hot) {
  import.meta.hot.dispose(() => root.unmount());
}
