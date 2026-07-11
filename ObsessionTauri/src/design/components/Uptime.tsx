import { useEffect, useRef, useState } from "react";
import { onRenderActiveChange, renderActive } from "../render";

// Живой таймер аптайма — тикает, пока обход/прокси активны. В трее/свёрнутом окне
// интервал паузится (как и все анимации): elapsed считается от абсолютного времени
// старта, поэтому при возврате значение мгновенно пересчитывается без потери точности.
export function Uptime({ active }: { active: boolean }) {
  const [elapsed, setElapsed] = useState(0);
  const startRef = useRef<number | null>(null);

  useEffect(() => {
    if (!active) {
      startRef.current = null;
      setElapsed(0);
      return;
    }
    startRef.current = Date.now();

    let id: ReturnType<typeof setInterval> | null = null;
    const tick = () => {
      if (startRef.current) {
        setElapsed(Math.floor((Date.now() - startRef.current) / 1000));
      }
    };
    const startTicking = () => {
      if (id != null) return;
      tick(); // сразу подтягиваем актуальное значение (без задержки до 1с)
      id = setInterval(tick, 1000);
    };
    const stopTicking = () => {
      if (id != null) {
        clearInterval(id);
        id = null;
      }
    };

    if (renderActive()) startTicking();
    const unsub = onRenderActiveChange((a) => (a ? startTicking() : stopTicking()));

    return () => {
      unsub();
      stopTicking();
    };
  }, [active]);

  if (!active) return null;

  const mm = String(Math.floor(elapsed / 60)).padStart(2, "0");
  const ss = String(elapsed % 60).padStart(2, "0");

  return (
    <div className="flex items-center gap-2 rounded-full bg-white/5 px-3 py-1.5">
      {/* Пульсирующий пинг-индикатор. */}
      <span className="relative flex h-2 w-2">
        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-ok opacity-75" />
        <span className="relative inline-flex h-2 w-2 rounded-full bg-ok" />
      </span>
      <span className="font-mono text-xs tabular-nums text-ink-soft">
        {mm}:{ss}
      </span>
    </div>
  );
}
