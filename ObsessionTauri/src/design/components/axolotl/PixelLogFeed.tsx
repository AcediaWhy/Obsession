import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useState } from "react";

import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import { dur, ease, spring, type LogLevel } from "../../tokens";

// Лента журнала по паттерну Magic UI «Animated List»: строки приходят по одной,
// въезжают снизу, стек переукладывается layout-анимацией, самая старая уходит.
// Раньше журнал лабы был шестью статичными строками и выглядел мёртвым.
// Оформление — прежнее стеклянное (sunken-lab-log), поменялось только поведение.
//
// Про цвет: LogLevel мы переиспользуем из design/tokens (общий словарь уровней),
// а `levelColor` оттуда — нет: цвет строки задаётся классами темы, чтобы журнал
// оставался в палитре лабы.

type LogRow = {
  id: number;
  time: string;
  source: string;
  message: string;
  level: LogLevel;
};

// Затравка отдаётся и на сервере: тест лабы рендерит разметку в node-окружении,
// и пустой журнал в SSR означал бы «в макете нет журнала».
const SEED: readonly Omit<LogRow, "id">[] = [
  { time: "09:17:04", source: "runtime", message: "защищённая служба готова", level: "success" },
  { time: "09:17:05", source: "route", message: "профиль general-alt2 выбран", level: "info" },
  { time: "09:17:05", source: "dns", message: "резолвер отвечает · 14 ms", level: "info" },
  { time: "09:17:06", source: "dpi", message: "youtube / twitch · доступно", level: "success" },
  { time: "09:17:06", source: "dpi", message: "discord · доступно", level: "success" },
  { time: "09:17:07", source: "core", message: "контур звезды стабилен", level: "info" },
];

const STREAM: readonly Omit<LogRow, "id" | "time">[] = [
  { source: "dpi", message: "квик-фейк отправлен · 3 позиции", level: "info" },
  { source: "route", message: "проверка обхода · 240 ms", level: "success" },
  { source: "dns", message: "кэш обновлён · 128 записей", level: "info" },
  { source: "core", message: "ядро держит поток", level: "success" },
  { source: "dpi", message: "повторная отправка · окно 2", level: "warn" },
  { source: "runtime", message: "служба отвечает на пинг", level: "info" },
];

function clockLabel() {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}

export function PixelLogFeed({
  phase,
  paused = false,
  limit = 7,
}: {
  phase: ObsessionVisualPhase;
  paused?: boolean;
  limit?: number;
}) {
  const [rows, setRows] = useState<LogRow[]>(() =>
    SEED.map((row, index) => ({ ...row, id: index })),
  );

  useEffect(() => {
    if (paused) return undefined;
    let next = SEED.length;
    const timer = window.setInterval(() => {
      const template = STREAM[next % STREAM.length];
      const id = next;
      next += 1;
      setRows((current) => [
        ...current.slice(Math.max(0, current.length - (limit - 1))),
        { ...template, id, time: clockLabel() },
      ]);
    }, 2400);
    return () => window.clearInterval(timer);
  }, [limit, paused]);

  const visible = rows.slice(-limit);

  return (
    <div className="sunken-lab-log flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between border-b border-glass-border px-5 py-4">
        <div>
          <div className="text-sm font-semibold text-ink">Журнал</div>
          <div className="text-2xs text-ink-muted">События службы в реальном времени</div>
        </div>
        <span className="flex items-center gap-2 text-2xs text-ok" data-paused={paused || undefined}>
          <i />
          {paused ? "hold" : "live"}
        </span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden p-4 font-mono text-2xs">
        <AnimatePresence initial={false}>
          {visible.map((row, index) => {
            const isLast = index === visible.length - 1;
            const faulted = phase === "fault" && isLast;
            return (
              <motion.div
                animate={{ opacity: 1, y: 0 }}
                className="sunken-lab-log__row"
                data-highlight={isLast || undefined}
                data-level={faulted ? "error" : row.level}
                exit={{ opacity: 0, y: -6, transition: { duration: dur.fast, ease: ease.exit } }}
                initial={{ opacity: 0, y: 10 }}
                key={row.id}
                layout
                transition={spring.rise}
              >
                <span>{row.time}</span>
                <b>{row.source}</b>
                <p>{faulted ? "маршрут потерян · повторный поиск" : row.message}</p>
              </motion.div>
            );
          })}
        </AnimatePresence>
        <div aria-hidden className="sunken-lab-log__cursor" />
      </div>
    </div>
  );
}

