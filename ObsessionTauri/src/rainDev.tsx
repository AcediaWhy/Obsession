// Временный дев-харнесс: монтирует только WebGL-сцену Rain без Tauri-рантайма.
// Открывается на /rain-dev.html из vite dev. Не попадает в прод-сборку
// (index.html его не подключает).
import { createRoot } from "react-dom/client";

import "./styles/globals.css";
import RainHybridScene from "./design/components/RainHybridScene";
import { RainFallback } from "./design/components/RainFallback";
import { RainWeatherModel, type RainWeatherSnapshot } from "./design/components/rain/weather";
import { useDpiStore } from "./store/dpiStore";

// Просмотр 2D-фолбэка вместо WebGL-сцены: /rain-dev.html?fallback=1
const showFallback = new URLSearchParams(window.location.search).get("fallback") === "1";

function Harness() {
  const active = useDpiStore((state) => state.active);
  return (
    <div className="fixed inset-0 bg-black">
      <div className="pointer-events-none absolute inset-0">
        {showFallback ? <RainFallback /> : <RainHybridScene />}
      </div>
      <button
        type="button"
        id="toggle-active"
        onClick={() => useDpiStore.setState({ active: !active })}
        className="absolute left-2 top-2 z-10 rounded bg-white/10 px-3 py-1 text-xs text-white"
      >
        {active ? "обход ВКЛ (шторм)" : "обход выкл (покой)"}
      </button>
    </div>
  );
}

// Старт сразу в шторме: /rain-dev.html?active=1
if (new URLSearchParams(window.location.search).get("active") === "1") {
  useDpiStore.setState({ active: true });
}

// Прогрев water map на 10 с шторма (headless/скриншот-проверки): ?warp=1
if (new URLSearchParams(window.location.search).get("warp") === "1") {
  const poll = setInterval(() => {
    const sim = (
      window as unknown as Record<
        string,
        { step: (dt: number, w: RainWeatherSnapshot) => void } | undefined
      >
    ).__rainSim;
    if (!sim) return;
    clearInterval(poll);
    const weather = new RainWeatherModel(true);
    for (let i = 0; i < 600; i += 1) {
      sim.step(1 / 60, weather.step(1 / 60, { active: true, reducedMotion: false }));
    }
  }, 100);
}

createRoot(document.getElementById("root")!).render(<Harness />);
