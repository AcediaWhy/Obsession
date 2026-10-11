import type { Zapret2HistoryRecord } from "../../lib/tauri";

function RecordDetails({ entry }: { entry: Zapret2HistoryRecord }) {
  const outcome = entry.outcome === "passed" ? (entry.kind === "scenario" ? entry.report.source === "player_observation"
    ? "видео проверено плеером" : "работает по твоей проверке" : "сетевые проверки пройдены")
    : ({ failed: "отказ", partial: "частичный результат", inconclusive: "не удалось оценить", insufficient: "недостаточно повторов",
      cancelled: "отменено", aborted: "прервано", exhausted: "подтверждённый профиль не найден", tie: "ничья" } as Record<string, string>)[entry.outcome] ?? entry.outcome;
  const service = ({ discord: "Discord", youtube_twitch: "YouTube", gaming: "Gaming + GitHub" } as Record<string, string>)[String(entry.report.category)]
    ?? "Проверка";
  return <p className="mb-2">{new Date(entry.recordedAtUnixMs).toLocaleString()} · {service} · {outcome}</p>;
}

export function Zapret2History({ records }: { records: Zapret2HistoryRecord[] }) {
  if (records.length === 0) return null;
  return <details className="text-xs text-ink-muted">
    <summary className="cursor-pointer">История испытаний · {records.length}</summary>
    <p className="my-2">Исторические результаты доступны для просмотра. Для применения требуется свежий турнир.</p>
    {[...records].reverse().map((entry, index) => <RecordDetails key={`${entry.recordedAtUnixMs}:${index}`} entry={entry} />)}
  </details>;
}
