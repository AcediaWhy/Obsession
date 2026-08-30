import { useState } from "react";
import ReactDOM from "react-dom/client";

import { CatnapSpriteLab } from "./design/components/CatnapSpriteLab";
import type { CatnapDetail } from "./design/components/CatnapSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import "./styles/fonts.css";
import "./styles/catnapLab.css";

const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const CORE_SIZES = [240, 104] as const;

const COPY: Record<ObsessionVisualPhase, { title: string; text: string }> = {
  idle: {
    title: "Золотой закатный сон",
    text: "Кот свернулся клубочком на теплом подоконнике вагона. Заходящее солнце греет пушистую спинку, а в воздухе кружатся золотые пылинки.",
  },
  engaging: {
    title: "Ленивое пробуждение",
    text: "Кончик хвоста слегка подрагивает, а солнце за окном вспыхивает теплым медовым светом.",
  },
  scanning: {
    title: "Сквозь дрему",
    text: "Теплый солнечный зайчик скользит по вагону, кот открывает один янтарный глаз.",
  },
  focused: {
    title: "Солнечный полдень",
    text: "Кот бодрствует в лучах заката, шерстка светится золотом, а сонное облачко Zzz растворяется.",
  },
  fault: {
    title: "Стук колес на стрелке",
    text: "Вагон качнуло на рельсах — кот вздрагивает и настороженно прижимает ушки.",
  },
};

function CatnapLab() {
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [coreSize, setCoreSize] = useState<(typeof CORE_SIZES)[number]>(240);
  const [detail, setDetail] = useState<CatnapDetail>("auto");
  const [paused, setPaused] = useState(false);
  const [fieldBackdrop, setFieldBackdrop] = useState(true);
  const active = phase === "focused";

  return (
    <main className="catnap-lab-shell">
      <section className="catnap-lab-stage" data-backdrop={fieldBackdrop ? "field" : "slate"}>
        <header className="catnap-lab-controls" aria-label="Catnap lab controls">
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
            <select value={detail} onChange={(event) => setDetail(event.target.value as CatnapDetail)}>
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

        <div className="catnap-lab-content">
          <section className="catnap-lab-hero">
            <div className="catnap-lab-copy">
              <span className="catnap-lab-kicker">CATNAP / PIXEL CORE LAB</span>
              <h1>Закат в окне<br />поезда</h1>
              <strong>{COPY[phase].title}</strong>
              <p>{COPY[phase].text}</p>
              <small>52×52 integer pixel grid · warm golden hour palette · sunset train sleeper</small>
            </div>

            <div className="catnap-lab-core-well">
              <button
                aria-label={active ? "Усыпить котика" : "Разбудить солнце"}
                className="catnap-lab-core-button"
                type="button"
                onClick={() => setPhase(active ? "idle" : "focused")}
                style={{ width: coreSize, height: coreSize }}
              >
                <CatnapSpriteLab detail={detail} phase={phase} paused={paused} size={coreSize} />
              </button>
              <span>{active ? "CLICK · SLEEP" : "CLICK · WAKE"}</span>
            </div>
          </section>

          <section className="catnap-lab-state-strip" aria-label="All animation states">
            {PHASES.map((item) => (
              <button
                className="catnap-lab-state-card"
                data-selected={item === phase || undefined}
                key={item}
                onClick={() => setPhase(item)}
                type="button"
              >
                <CatnapSpriteLab detail="base" phase={item} paused={paused} size={104} />
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
root.render(<CatnapLab />);

if (import.meta.hot) {
  import.meta.hot.dispose(() => root.unmount());
}
