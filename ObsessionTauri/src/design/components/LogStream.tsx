import { useEffect, useRef } from "react";
import { useLogStore } from "../../store/logStore";
import { levelColor } from "../tokens";

// Моно-лог реального времени с автоскроллом.
export function LogStream({ height = 200 }: { height?: number }) {
  const lines = useLogStore((s) => s.lines);
  const clear = useLogStore((s) => s.clear);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [lines]);

  return (
    <div className="flex flex-col">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-2xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
          Лог
        </span>
        <button
          onClick={clear}
          className="no-drag text-2xs text-ink-muted transition-colors hover:text-ink-soft"
        >
          очистить
        </button>
      </div>
      <div
        ref={ref}
        style={{ height }}
        className="overflow-y-auto rounded-xl border border-glass-border bg-black/30 p-3 font-mono text-2xs tabular-nums leading-relaxed"
      >
        {lines.length === 0 && (
          <div className="text-ink-muted">Лог пуст. Действия появятся здесь.</div>
        )}
        {lines.map((l, i) => (
          <div key={i} className="flex gap-2">
            <span className="shrink-0 text-ink-muted">{l.ts}</span>
            <span style={{ color: levelColor[l.level] }} className="break-all">
              {l.message}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
