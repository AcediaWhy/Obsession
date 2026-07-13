import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { onRenderActiveChange, renderActive } from "../render";
import { dur, ease, spring } from "../tokens";

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

  const mm = String(Math.floor(elapsed / 60)).padStart(2, "0");
  const ss = String(elapsed % 60).padStart(2, "0");

  // Появление/уход — пружиной, не телепортом. layout — чтобы соседи (StatusBadge
  // в шапках Dpi/Telegram, у них тоже layout) раздвигались тем же движением.
  // initial={false} — при открытии экрана с уже активной защитой пилюля не
  // «выпрыгивает», а просто есть.
  return (
    <AnimatePresence initial={false}>
      {active && (
        <motion.div
          layout
          initial={{ opacity: 0, scale: 0.85 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: 0.9, transition: { duration: dur.fast, ease: ease.exit } }}
          transition={spring.expand}
          className="flex items-center gap-2 rounded-full bg-white/5 px-3 py-1.5"
        >
          {/* Пульсирующий пинг-индикатор. */}
          <span className="relative flex h-2 w-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-ok opacity-75" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-ok" />
          </span>
          <span className="font-mono text-xs tabular-nums text-ink-soft">
            {mm}:{ss}
          </span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
