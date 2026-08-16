import { useState } from "react";
import { createRoot } from "react-dom/client";

import "./styles/globals.css";
import { ObsessionCore } from "./design/components/ObsessionCore";
import { ObsessionFallback } from "./design/components/ObsessionFallback";
import { ObsessionField } from "./design/components/ObsessionField";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";

const PHASES: ObsessionVisualPhase[] = [
  "idle",
  "engaging",
  "scanning",
  "focused",
  "fault",
];
const SCREENS = ["overview", "dpi", "telegram"] as const;

function Harness() {
  const params = new URLSearchParams(window.location.search);
  const requestedPhase = params.get("phase") as ObsessionVisualPhase | null;
  const requestedScreen = params.get("screen") as (typeof SCREENS)[number] | null;
  const [phase, setPhase] = useState<ObsessionVisualPhase>(
    requestedPhase && PHASES.includes(requestedPhase) ? requestedPhase : "idle",
  );
  const [screen, setScreen] = useState<(typeof SCREENS)[number]>(
    requestedScreen && SCREENS.includes(requestedScreen) ? requestedScreen : "overview",
  );
  const [paused, setPaused] = useState(params.get("paused") === "1");
  const fallback = params.get("fallback") === "1";
  const active = phase === "focused";
  const busy = phase === "engaging";
  const scanning = phase === "scanning";
  const alarm = phase === "fault";

  return (
    <div data-theme="obsession" className="fixed inset-0 overflow-hidden bg-[#030305] text-ink">
      {fallback ? (
        <ObsessionFallback phase={phase} screen={screen} paused={paused} />
      ) : (
        <ObsessionField phase={phase} screen={screen} paused={paused} />
      )}

      <div className="absolute inset-x-0 top-0 z-20 flex h-11 items-center justify-between border-b border-white/[0.05] bg-black/20 px-5 backdrop-blur-md">
        <div>
          <span className="font-display text-sm font-semibold tracking-wide">Obsession</span>
          <span className="ml-2 font-mono text-3xs uppercase tracking-[0.2em] text-ink-muted">The Fixation</span>
        </div>
        <div className="flex items-center gap-1.5">
          {SCREENS.map((item) => (
            <button
              key={item}
              type="button"
              onClick={() => setScreen(item)}
              className={`rounded-lg px-2.5 py-1.5 text-xs capitalize transition-colors ${screen === item ? "bg-white/10 text-ink" : "text-ink-muted hover:text-ink"}`}
            >
              {item}
            </button>
          ))}
        </div>
      </div>

      <main
        className="absolute inset-0 top-11 z-10 grid gap-5 p-5"
        style={{ gridTemplateColumns: "220px minmax(0, 1fr)" }}
      >
        <aside className="glass rounded-xl2 p-4">
          <div className="text-3xs font-semibold uppercase tracking-[0.18em] text-ink-muted">Visual phase</div>
          <div className="mt-3 flex flex-col gap-1.5">
            {PHASES.map((item) => (
              <button
                key={item}
                type="button"
                onClick={() => setPhase(item)}
                className={`rounded-xl px-3 py-2 text-left text-xs transition-colors ${phase === item ? "nav-active-plate border border-accent/40 text-ink" : "text-ink-soft hover:bg-white/5"}`}
              >
                {item}
              </button>
            ))}
          </div>
          <button
            type="button"
            onClick={() => setPaused((value) => !value)}
            className="mt-4 w-full rounded-xl border border-white/10 px-3 py-2 text-xs text-ink-soft hover:bg-white/5"
          >
            {paused ? "Resume renderer" : "Pause / stop-frame"}
          </button>
        </aside>

        <section
          className="grid min-w-0 gap-5"
          style={{ gridTemplateColumns: "minmax(0, 1fr) 280px" }}
        >
          <div className="glass spotlight relative overflow-hidden rounded-xl2 p-6">
            <div className="spotlight-ring pointer-events-none absolute inset-0 rounded-[inherit]" />
            <div className="relative z-10 max-w-xl">
              <div className="text-3xs font-semibold uppercase tracking-[0.2em] text-ink-muted">{screen}</div>
              <h1 className="text-gradient mt-2 font-display text-4xl font-semibold">Point of fixation</h1>
              <p className="mt-3 max-w-lg text-sm leading-6 text-ink-soft">
                Black optical glass gathers pearl light around a controlled carmine core.
                The scene reacts to runtime state without changing screen geometry.
              </p>
            </div>
            <div className="absolute bottom-5 left-6 flex gap-3">
              <button type="button" className="rounded-xl bg-accent px-4 py-2 text-sm font-semibold text-[#05060B]">Primary action</button>
              <button type="button" className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm text-ink-soft">Details</button>
            </div>
          </div>

          <div className="flex flex-col items-center justify-center gap-5">
            <ObsessionCore
              active={active}
              busy={busy}
              scanning={scanning}
              alarm={alarm}
              paused={paused}
              onClick={() => setPhase(active ? "idle" : "focused")}
              size={230}
            />
            <div className="glass w-full rounded-xl2 p-4 text-center">
              <div className="font-mono text-2xs uppercase tracking-[0.18em] text-ink-muted">phase</div>
              <div className="mt-1 font-display text-lg font-semibold text-ink">{phase}</div>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Harness />);
