import { useEffect, useRef } from "react";

import { useLogStore } from "../../store/logStore";
import { useRenderHidden } from "../render";
import { levelColor } from "../tokens";

const MAX_DOM_LINES = 120;
const MAX_RENDERED_MESSAGE_CHARS = 700;
const BOTTOM_THRESHOLD_PX = 24;

// Журнал с пакетным обновлением строк и автопрокруткой у нижней границы.
export function LogStream({ height }: { height?: number }) {
  const linesRaw = useLogStore((state) => state.lines);
  const clear = useLogStore((state) => state.clear);
  // В скрытом окне сохраняем последнюю видимую версию журнала.
  // После показа берём актуальные строки из хранилища.
  const hidden = useRenderHidden();
  const frozenLines = useRef(linesRaw);
  if (!hidden) frozenLines.current = linesRaw;
  const lines = hidden ? frozenLines.current : linesRaw;
  const ref = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const autoScrollRef = useRef(false);
  const visibleLines =
    lines.length > MAX_DOM_LINES ? lines.slice(lines.length - MAX_DOM_LINES) : lines;
  const hiddenLineCount = lines.length - visibleLines.length;
  const lastLineId = lines[lines.length - 1]?.id ?? 0;

  useEffect(() => {
    const node = ref.current;
    if (!node || !stickToBottomRef.current) return;
    autoScrollRef.current = true;
    const frame = window.requestAnimationFrame(() => {
      node.scrollTop = node.scrollHeight;
      autoScrollRef.current = false;
    });
    return () => {
      window.cancelAnimationFrame(frame);
      autoScrollRef.current = false;
    };
  }, [lastLineId]);

  const onScroll = () => {
    const node = ref.current;
    if (!node || autoScrollRef.current) return;
    const distanceToBottom = node.scrollHeight - node.scrollTop - node.clientHeight;
    stickToBottomRef.current = distanceToBottom <= BOTTOM_THRESHOLD_PX;
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-2xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
          Лог
        </span>
        <button
          onClick={clear}
          className="no-drag theme-morph text-2xs text-ink-muted transition-colors hover:text-ink-soft"
        >
          очистить
        </button>
      </div>
      <div
        ref={ref}
        onScroll={onScroll}
        style={{ height }}
        className={`scroll-fade overflow-y-auto rounded-xl border border-glass-border bg-black/30 p-3 font-mono text-2xs tabular-nums leading-relaxed ${
          height === undefined ? "min-h-0 flex-1" : ""
        }`}
      >
        {lines.length === 0 && (
          <div className="text-ink-muted">Лог пуст. Действия появятся здесь.</div>
        )}
        {hiddenLineCount > 0 && (
          <div className="log-row text-ink-muted">
            скрыто предыдущих строк: {hiddenLineCount}
          </div>
        )}
        {visibleLines.map((line) => (
          <div key={line.id} className="log-row flex gap-2 rounded-sm">
            <span className="shrink-0 text-ink-muted">{line.ts}</span>
            <span
              style={{ color: levelColor[line.level] }}
              className="min-w-0 flex-1 whitespace-pre-wrap [overflow-wrap:anywhere]"
            >
              {line.message.length > MAX_RENDERED_MESSAGE_CHARS
                ? `${line.message.slice(0, MAX_RENDERED_MESSAGE_CHARS)}… (полная запись в app.log)`
                : line.message}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
