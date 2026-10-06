import { useState } from "react";
import type { Zapret2HistoryRecord } from "../../lib/tauri";

function RecordDetails({ entry }: { entry: Zapret2HistoryRecord }) {
  const [open, setOpen] = useState(false);
  const outcome = entry.outcome === "passed" ? (entry.kind === "scenario" ? "работает по наблюдению пользователя" : "сетевые пробы пройдены")
    : ({ failed: "отказ", partial: "частичный результат", inconclusive: "не удалось оценить", insufficient: "недостаточно повторов",
      cancelled: "отменено", aborted: "прервано", exhausted: "подтверждённый профиль не найден", tie: "ничья" } as Record<string, string>)[entry.outcome] ?? entry.outcome;
  return <details className="mb-2" onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary className="cursor-pointer">{new Date(entry.recordedAtUnixMs).toLocaleString()} · {entry.subject} · {outcome}</summary>
    {open && <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(entry.report, null, 2)}</pre>}
  </details>;
}

export function Zapret2History({ records }: { records: Zapret2HistoryRecord[] }) {
  if (records.length === 0) return null;
  return <details className="text-xs text-ink-muted">
    <summary className="cursor-pointer">История испытаний · {records.length}</summary>
    <p className="my-2">Исторические результаты доступны для просмотра. Для применения требуется свежий турнир.</p>
    {[...records].reverse().map((entry, index) => <RecordDetails key={`${entry.recordedAtUnixMs}:${index}`} entry={entry} />)}
  </details>;
}
