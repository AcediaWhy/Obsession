import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { Theme } from "../store/themeStore";

function Leaf({ index }: { index: number }) {
  return (
    <svg className={`overview-fx-leaf overview-fx-leaf-${index}`} viewBox="0 0 32 38">
      <path d="m16 2 4 9 6-3-1 9 6 2-9 7-5 2-1 8-2-1 1-8-9-3-5-7 8 1-2-9 7 4Z" fill="currentColor" />
      <path d="m16 10-1 21m0-10-6-4m6 7 8-7" fill="none" stroke="#64381c" strokeWidth="1.1" />
    </svg>
  );
}

export function OverviewOrnament({ theme }: { theme: Theme }) {
  return (
    <span className="overview-fx-ornament" aria-hidden="true">
      {theme === "goldenmeadow" ? <><Leaf index={0} /><Leaf index={1} /><span className="overview-fx-pressed-leaf">❧</span></> :
        theme === "ophanim" ? <><span className="overview-fx-seal">✧</span><i className="overview-fx-spark" /><i className="overview-fx-spark overview-fx-second" /></> :
        theme === "japan" ? <><i className="overview-fx-drop" /><i className="overview-fx-drop overview-fx-second" /></> :
        theme === "fallendown" ? <><span className="overview-fx-save">✦</span><i className="overview-fx-pixel" /><i className="overview-fx-pixel overview-fx-second" /></> :
        theme === "yanineko" ? <>{[0, 1, 2].map((i) => <svg key={i} className="overview-fx-paw" style={{ "--i": i } as CSSProperties} viewBox="0 0 30 30"><ellipse cx="15" cy="20" rx="8" ry="6" /><ellipse cx="5" cy="12" rx="3" ry="4" /><ellipse cx="12" cy="7" rx="3" ry="4" /><ellipse cx="20" cy="8" rx="3" ry="4" /><ellipse cx="26" cy="14" rx="3" ry="4" /></svg>)}</> :
        <span className="overview-fx-light" />}
    </span>
  );
}

export function OverviewStatusEffect({ theme, event, disabled }: { theme: Theme; event: string; disabled: boolean }) {
  const previous = useRef(event);
  const [generation, setGeneration] = useState(0);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const changed = previous.current !== event;
    previous.current = event;
    if (disabled || !changed) {
      setVisible(false);
      return;
    }
    setGeneration((value) => value + 1);
    setVisible(true);
    // Слой удаляется после затухания, включая эффекты с задержкой запуска.
    const timeout = window.setTimeout(() => setVisible(false), 4600);
    return () => window.clearTimeout(timeout);
  }, [event, disabled]);

  return visible ? <span key={generation} className="overview-fx-event" aria-hidden="true"><OverviewOrnament theme={theme} /></span> : null;
}
