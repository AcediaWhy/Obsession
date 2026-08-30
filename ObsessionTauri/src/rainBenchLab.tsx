import { useState } from "react";
import ReactDOM from "react-dom/client";

import { RainUmbrellaSpriteLab } from "./design/components/RainUmbrellaSpriteLab";
import type { RainUmbrellaDetail } from "./design/components/RainUmbrellaSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/fonts.css";
import "./styles/rainBenchLab.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const CORE_SIZES = [240, 104] as const;

const COPY: Record<ObsessionVisualPhase, { title: string; text: string }> = {
  idle: {
    title: "Дождь снаружи",
    text: "Кот свернулся клубком под старым зонтом, а бумажный кораблик покачивается рядом в луже. С краёв купола медленно срываются капли.",
  },
  engaging: {
    title: "Зонт дрогнул",
    text: "Дождь усиливается. Кот просыпается, а большой зонт едва заметно покачивается под тяжестью воды.",
  },
  scanning: {
    title: "Кораблик на месте",
    text: "Кот приподнимает голову и прислушивается, не унесёт ли следующий порыв бумажный кораблик.",
  },
  focused: {
    title: "Сухо и спокойно",
    text: "Хвост плотнее обнимает лапы, кораблик покачивается в тёплом отражении, а дождь остаётся по другую сторону зонта.",
  },
  fault: {
    title: "Порыв ветра",
    text: "Зонт и ушки вздрагивают на один пиксель. Кот широко открывает глаза, но кораблик остаётся рядом.",
  },
};

function RainBenchLab() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(240);
  const [detail, setDetail] = useState<RainUmbrellaDetail>("auto");
  const [paused, setPaused] = useState(false);
  const [photoBackdrop, setPhotoBackdrop] = useState(true);

  const active = phase === "focused";

  return (
    <main className="rain-bench-lab-shell">
      <section className="rain-bench-lab-stage" data-backdrop={photoBackdrop ? "photo" : "slate"}>
        <div className="rain-bench-lab-atmosphere" />

        <header className="rain-bench-lab-controls" aria-label="Rain bench lab controls">
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
            <select value={detail} onChange={(event) => setDetail(event.target.value as RainUmbrellaDetail)}>
              <option value="auto">auto</option>
              <option value="base">base</option>
              <option value="hero">hero</option>
            </select>
          </label>
          <button type="button" data-on={paused || undefined} onClick={() => setPaused((value) => !value)}>
            {paused ? "Motion: paused" : "Motion: running"}
          </button>
          <button type="button" data-on={!photoBackdrop || undefined} onClick={() => setPhotoBackdrop((value) => !value)}>
            {photoBackdrop ? "Backdrop: photo" : "Backdrop: slate"}
          </button>
        </header>

        <div className="rain-bench-lab-content">
          <section className="rain-bench-lab-hero">
            <div className="rain-bench-lab-copy">
              <span className="rain-bench-lab-kicker">RAIN / PIXEL CORE STUDY 02</span>
              <h1>Пока идёт<br />дождь</h1>
              <strong>{COPY[phase].title}</strong>
              <p>{COPY[phase].text}</p>
              <small>120×120 hero grid · 52×52 preview grid · transparent standalone sprite</small>
            </div>

            <div className="rain-bench-lab-core-well">
              <button
                aria-label={active ? "Вернуть тихий дождь" : "Включить сильный дождь"}
                className="rain-bench-lab-core-button"
                type="button"
                onClick={() => setPhase(active ? "idle" : "focused")}
                style={{ width: coreSize, height: coreSize }}
              >
                <RainUmbrellaSpriteLab detail={detail} phase={phase} paused={paused} size={coreSize} />
              </button>
              <span>{active ? "CLICK · QUIET" : "CLICK · RAIN"}</span>
            </div>
          </section>

          <section className="rain-bench-lab-state-strip" aria-label="All animation states">
            {PHASES.map((item) => (
              <button
                className="rain-bench-lab-state-card"
                data-selected={item === phase || undefined}
                key={item}
                onClick={() => setPhase(item)}
                type="button"
              >
                <RainUmbrellaSpriteLab detail="base" phase={item} paused={paused} size={104} />
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
root.render(<RainBenchLab />);

if (import.meta.hot) {
  import.meta.hot.dispose(() => root.unmount());
}
