import { useState } from "react";
import ReactDOM from "react-dom/client";

import { AuroraCatSpriteLab } from "./design/components/AuroraCatSpriteLab";
import type { AuroraCatDetail } from "./design/components/AuroraCatSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/fonts.css";
import "./styles/auroraCatLab.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const CORE_SIZES = [240, 104] as const;

const COPY: Record<ObsessionVisualPhase, { title: string; text: string }> = {
  idle: {
    title: "Полярная тишина",
    text: "Кот сидит на снегу и смотрит на маленькую звезду. Его хвост медленно растворяется в холодном сиянии.",
  },
  engaging: {
    title: "Сияние просыпается",
    text: "Ушки приподнимаются, а по цельной ленте хвоста проходит первая яркая волна.",
  },
  scanning: {
    title: "Звезда найдена",
    text: "Глаз загорается бирюзой, и маленькая искра скользит вдоль края северного сияния.",
  },
  focused: {
    title: "Небо отвечает",
    text: "Холодные оттенки теплеют до магенты, но силуэт хвоста остаётся спокойным и цельным.",
  },
  fault: {
    title: "Солнечный всплеск",
    text: "Кот вздрагивает и прижимает ушки, пока сияние на мгновение становится слишком ярким.",
  },
};

function AuroraCatLab() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(240);
  const [detail, setDetail] = useState<AuroraCatDetail>("auto");
  const [paused, setPaused] = useState(false);
  const [fieldBackdrop, setFieldBackdrop] = useState(true);
  const active = phase === "focused";

  return (
    <main className="aurora-cat-lab-shell">
      <section className="aurora-cat-lab-stage" data-backdrop={fieldBackdrop ? "field" : "slate"}>
        <div className="aurora-cat-lab-atmosphere" />

        <header className="aurora-cat-lab-controls" aria-label="Aurora cat lab controls">
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
            <select value={detail} onChange={(event) => setDetail(event.target.value as AuroraCatDetail)}>
              <option value="auto">auto</option>
              <option value="base">base</option>
              <option value="hero">hero</option>
            </select>
          </label>
          <button type="button" data-on={paused || undefined} onClick={() => setPaused((value) => !value)}>
            {paused ? "Motion: paused" : "Motion: running"}
          </button>
          <button type="button" data-on={!fieldBackdrop || undefined} onClick={() => setFieldBackdrop((value) => !value)}>
            {fieldBackdrop ? "Backdrop: field" : "Backdrop: slate"}
          </button>
        </header>

        <div className="aurora-cat-lab-content">
          <section className="aurora-cat-lab-hero">
            <div className="aurora-cat-lab-copy">
              <span className="aurora-cat-lab-kicker">AURORA / PIXEL CORE STUDY 01</span>
              <h1>Пока небо<br />светится</h1>
              <strong>{COPY[phase].title}</strong>
              <p>{COPY[phase].text}</p>
              <small>120×120 hero grid · 52×52 preview grid · transparent standalone sprite</small>
            </div>

            <div className="aurora-cat-lab-core-well">
              <button
                aria-label={active ? "Вернуть полярную тишину" : "Разбудить сияние"}
                className="aurora-cat-lab-core-button"
                type="button"
                onClick={() => setPhase(active ? "idle" : "focused")}
                style={{ width: coreSize, height: coreSize }}
              >
                <AuroraCatSpriteLab detail={detail} phase={phase} paused={paused} size={coreSize} />
              </button>
              <span>{active ? "CLICK · QUIET" : "CLICK · AURORA"}</span>
            </div>
          </section>

          <section className="aurora-cat-lab-state-strip" aria-label="All animation states">
            {PHASES.map((item) => (
              <button
                className="aurora-cat-lab-state-card"
                data-selected={item === phase || undefined}
                key={item}
                onClick={() => setPhase(item)}
                type="button"
              >
                <AuroraCatSpriteLab detail="base" phase={item} paused={paused} size={104} />
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
root.render(<AuroraCatLab />);

if (import.meta.hot) {
  import.meta.hot.dispose(() => root.unmount());
}
