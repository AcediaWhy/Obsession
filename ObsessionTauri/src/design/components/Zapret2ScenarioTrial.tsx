import { useEffect, useState } from "react";
import type { Zapret2Observation, Zapret2Scenario, Zapret2ScenarioProgress, Zapret2ScenarioResult } from "../../lib/tauri";
import { Button } from "./atoms";

export function Zapret2ScenarioActions({ category, transport, disabled, test }: {
  category: string; transport: string; disabled: boolean; test: (scenario: Zapret2Scenario) => void;
}) {
  const discord = category === "discord" && transport === "tls";
  const youtube = category === "youtube_twitch" && ["tls", "quic"].includes(transport);
  if (!discord && !youtube) return null;
  return <div className="flex flex-col gap-1">
    <p className="text-ink-muted">На время проверки включим найденные настройки, затем вернём предыдущие.</p>
    <div className="flex flex-wrap gap-2">
      {discord && <><Button variant="ghost" disabled={disabled} onClick={() => test("discord_startup")}>Проверить запуск Discord</Button>
        <Button variant="ghost" disabled={disabled} onClick={() => test("discord_voice")}>Проверить голос</Button></>}
      {youtube && <Button variant="ghost" disabled={disabled} onClick={() => test("youtube_playback")}>Проверить видео</Button>}
    </div>
    {youtube && <details className="text-ink-muted"><summary className="cursor-pointer">Проверка вручную</summary>
      <Button variant="ghost" disabled={disabled} onClick={() => test("youtube_video")}>Проверить в своём браузере</Button>
    </details>}
  </div>;
}

export function Zapret2ScenarioTrial({ progress, cancelling, observing, observe, cancel }: {
  progress: Zapret2ScenarioProgress; cancelling: boolean; observing: boolean;
  observe: (outcome: Zapret2Observation) => void; cancel: () => void;
}) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 500); return () => clearInterval(timer); }, []);
  const wait = Math.max(0, Math.ceil((progress.confirmAfterUnixMs - now) / 1000));
  const remaining = progress.expiresUnixMs > 0 ? Math.max(0, Math.ceil((progress.expiresUnixMs - now) / 1000)) : null;
  const ready = progress.phase === "step" && !!progress.stepId && !cancelling && !observing && remaining !== 0;
  return <div className="flex flex-col gap-2 rounded-xl border border-accent/25 bg-accent/5 p-3 text-xs" aria-label="Испытание в приложении">
    <p className="font-semibold text-ink" role="status">{cancelling || progress.phase === "restoring" ? "Возвращаем исходный профиль…" : progress.label}</p>
    <p className="text-ink-soft">{progress.instruction}</p>
    <p className="text-ink-muted">Шаг {progress.stepIndex} / {progress.total || "…"}{remaining !== null ? ` · осталось ${remaining} с` : ""}</p>
    {progress.phase === "step" && <div className="flex flex-wrap gap-2">
      <Button variant="ghost" disabled={!ready || wait > 0} onClick={() => observe("passed")}>{wait > 0 ? `Подтвердить через ${wait} с` : "Шаг работает"}</Button>
      <Button variant="danger" disabled={!ready} onClick={() => observe("failed")}>Не работает</Button>
      <Button variant="ghost" disabled={!ready} onClick={() => observe("inconclusive")}>Не удалось проверить</Button>
    </div>}
    <Button variant="ghost" disabled={cancelling || progress.phase === "restoring"} onClick={cancel}>Отмена и возврат</Button>
    <details className="text-ink-muted"><summary className="cursor-pointer">Подробности проверки</summary>
      <p className="mt-2">{progress.scenario === "youtube_playback"
        ? "Отдельный плеер проверяет загрузку из сети, воспроизведение и перемотку. Результат относится ко всей конфигурации; обработка конкретным блоком DPI не устанавливается. Лимит — 60 секунд, после проверки окно закрывается."
        : "Результат подтверждаешь ты. Проверяется вся временная конфигурация; транспорт браузера и исполнение действий DPI не устанавливаются."}</p>
    </details>
  </div>;
}

const labels: Record<Zapret2ScenarioResult["scenario"], string> = {
  discord_startup: "Запуск Discord", discord_voice: "Голос Discord", youtube_video: "Видео YouTube", youtube_playback: "Видео YouTube",
};
const outcomes: Record<string, string> = { passed: "работает по твоей проверке", failed: "отказ по твоей проверке",
  inconclusive: "не удалось проверить", cancelled: "отменено", aborted: "испытание прервано" };
export function Zapret2ScenarioResults({ results }: { results: Zapret2ScenarioResult[] }) {
  if (results.length === 0) return null;
  return <div className="flex flex-col gap-1 text-xs">
    {results.map((result) => <details key={result.trialId} className="text-ink-muted">
      <summary className="cursor-pointer">{labels[result.scenario]}: {result.source === "player_observation"
        ? ({ passed: "воспроизведение подтверждено", failed: "воспроизведение прерывается", inconclusive: "не удалось проверить",
          cancelled: "отменено", aborted: "испытание прервано" }[result.status]) : outcomes[result.status]}</summary>
      <p className="mt-1">{result.source === "player_observation"
        ? "Источник — наблюдения тестового плеера и загрузки из сети. Результат относится ко всей конфигурации; работа конкретного TLS/QUIC-профиля не установлена."
        : "Источник — твои наблюдения. Транспорт не определён; результат относится ко всей испытанной конфигурации."}</p>
      {!result.contextValid && <p>Результат не подтверждает текущую конфигурацию: контекст изменился или возврат не подтверждён.</p>}
      {result.failure && <p>{result.failure}</p>}
      {result.historyWarning && <p>{result.historyWarning}</p>}
      {result.measurement.functions.map((entry) => <p key={entry.function}>{entry.label}: {result.source === "player_observation"
        ? ({ passed: "пройдено", failed: "отказ", inconclusive: "не удалось проверить", cancelled: "отменено", not_tested: "не проверено" }[entry.outcome])
        : outcomes[entry.outcome] ?? "не проверено"}{entry.reason ? ` · ${entry.reason}` : ""}</p>)}
      {result.playback && <details className="mt-1"><summary className="cursor-pointer">Подробности плеера</summary>
        <p>Медиаданные из сети: {Math.round(result.playback.mediaBytes / 1024)} КБ · воспроизведение: {result.playback.playbackSeconds.toFixed(1)} с</p>
        <p>Кадров: {result.playback.decodedFrames} · остановки: {result.playback.stallSeconds.toFixed(1)} с · {result.playback.width} × {result.playback.height}</p>
        <p>Перемотка: {result.playback.seekConfirmed ? "проверена" : "не проверена"} · завершение: {result.playback.stopReason}</p>
      </details>}
    </details>)}
  </div>;
}
