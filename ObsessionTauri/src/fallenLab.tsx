import { useState } from "react";
import ReactDOM from "react-dom/client";

import { FallenSpriteLab } from "./design/components/FallenSpriteLab";
import type { FallenDetail } from "./design/components/FallenSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/fonts.css";
import "./styles/fallenLab.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const CORE_SIZES = [240, 104] as const;

const COPY: Record<ObsessionVisualPhase, { title: string; text: string }> = {
  idle: {
    title: "hOI! Дрема в золотых цветах",
    text: "Кошечка Темми сладко спит на полянке золотых лютиков под мерцанием пещерных звезд Водопада.",
  },
  engaging: {
    title: "hOI! Пробуждение в деревне Темми",
    text: "Алое сердце души начинает мягко биться в такт, золотые звезды сохранения озаряют пещеру.",
  },
  scanning: {
    title: "hOI! Интенсивная вибрация!",
    text: "Темми трясется от восторга (hOI!!), ушки подергиваются, сканируя эфир на предмет хлопьев Темми.",
  },
  focused: {
    title: "РЕШИМОСТЬ (DETERMINATION)",
    text: "Алое сердце души ярко пылает чистой решимостью! Глазки Темми сияют золотыми искрами.",
  },
  fault: {
    title: "NO colleg?! Ночная тревога",
    text: "Плата за колледж не внесена или обнаружен сбой в сети — Темми в панике прячется в коробку!",
  },
};

function FallenLab() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(240);
  const [detail, setDetail] = useState<FallenDetail>("auto");
  const [paused, setPaused] = useState(false);
  const active = phase === "focused";

  return (
    <main className="fallen-lab-shell">
      <section className="fallen-lab-stage">
        <header className="fallen-lab-controls" aria-label="Fallen Down lab controls">
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
            <select value={detail} onChange={(event) => setDetail(event.target.value as FallenDetail)}>
              <option value="auto">auto</option>
              <option value="base">base</option>
              <option value="hero">hero</option>
            </select>
          </label>
          <button type="button" onClick={() => setPaused((value) => !value)}>
            {paused ? "Motion: paused" : "Motion: running"}
          </button>
        </header>

        <div className="fallen-lab-content">
          <section className="fallen-lab-hero">
            <div className="fallen-lab-copy">
              <span className="fallen-lab-kicker">FALLEN DOWN / TEMMIE PIXEL LAB</span>
              <h1>Кошечка Темми<br />и алая душа</h1>
              <strong>{COPY[phase].title}</strong>
              <p>{COPY[phase].text}</p>
              <small>64×52 integer pixel grid · Undertale Determination & Temmie Village</small>
            </div>

            <div className="fallen-lab-core-well">
              <button
                aria-label={active ? "Усыпить Темми" : "Наполнить Темми РЕШИМОСТЬЮ"}
                className="fallen-lab-core-button"
                type="button"
                onClick={() => setPhase(active ? "idle" : "focused")}
                style={{ width: coreSize, height: coreSize }}
              >
                <FallenSpriteLab detail={detail} phase={phase} paused={paused} size={coreSize} />
              </button>
              <span>{active ? "DETERMINATION · ON" : "CLICK · hOI!"}</span>
            </div>
          </section>

          <section className="fallen-lab-state-strip" aria-label="All animation states">
            {PHASES.map((item) => (
              <button
                className="fallen-lab-state-card"
                data-selected={item === phase || undefined}
                key={item}
                onClick={() => setPhase(item)}
                type="button"
              >
                <FallenSpriteLab detail="base" phase={item} paused={paused} size={104} />
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
root.render(<FallenLab />);

if (import.meta.hot) {
  import.meta.hot.dispose(() => root.unmount());
}
