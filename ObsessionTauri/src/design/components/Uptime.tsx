import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { onRenderActiveChange, renderActive } from "../render";
import { dur, ease, spring } from "../tokens";

// Показывает время работы активного обхода или прокси. В скрытом окне интервал
// останавливается; при возвращении значение пересчитывается по времени старта.
// startedAt приходит из backend в миллисекундах Unix. Если его нет, используем
// время монтирования компонента при active=true.
export function Uptime({
  active,
  startedAt,
}: {
  active: boolean;
  startedAt?: number | null;
}) {
  const [elapsed, setElapsed] = useState(0);
  const startRef = useRef<number | null>(null);

  useEffect(() => {
    if (!active) {
      startRef.current = null;
      setElapsed(0);
      return;
    }
    // Приоритет — backend-время старта; иначе фиксируем момент mount.
    startRef.current = startedAt ?? Date.now();

    let id: ReturnType<typeof setInterval> | null = null;
    const tick = () => {
      if (startRef.current) {
        setElapsed(Math.max(0, Math.floor((Date.now() - startRef.current) / 1000)));
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
  }, [active, startedAt]);

  const mm = String(Math.floor(elapsed / 60)).padStart(2, "0");
  const ss = String(elapsed % 60).padStart(2, "0");

  // layout синхронизирует сдвиг соседних элементов. initial={false} отключает
  // входную анимацию при открытии экрана с уже активной защитой.
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
