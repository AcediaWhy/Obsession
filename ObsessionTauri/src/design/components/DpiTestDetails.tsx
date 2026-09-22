import type { DpiTestReport } from "../../lib/tauri";

export function testSummary(report: DpiTestReport): string {
  return { passed: "проверки пройдены", partial: "частично", failed: "доступ не подтверждён", cancelled: "отменено" }[report.status];
}

function endpointName(url: string): string {
  if (url.includes("/api/v10/gateway")) return "Discord API";
  if (url.includes("updates.discord.com")) return "Обновления Discord";
  if (url.includes("cdn.discordapp.com")) return "Картинка Discord CDN";
  if (url.includes("ytimg.com")) return "Превью YouTube";
  if (url.includes("youtube.com")) return "Страница YouTube";
  if (url.includes("twitch.tv")) return "Страница Twitch";
  return new URL(url).hostname;
}

export function DpiTestDetails({ report }: { report: DpiTestReport }) {
  return <details className="text-xs text-ink-soft">
    <summary className="cursor-pointer py-1">Подробности проверки</summary>
    <div className="mt-2 flex flex-col gap-2">
      {report.checks.map((check) => <div key={check.url} className="break-words">
        <div className={check.passed ? "text-ok" : "text-ink-soft"}>
          {endpointName(check.url)}: {check.passed ? "загружено" : "не подтверждено"}
        </div>
        <div>{(check.elapsed_ms / 1000).toFixed(2)} с · {check.bytes} байт · попыток: {check.attempts}
          {check.http_status != null ? ` · HTTP ${check.http_status}` : ""}</div>
        {check.error && <div className="mt-1 break-all">{check.error}</div>}
      </div>)}
      <p className="text-3xs">Время включает подключение и повторы, это не замер скорости канала. Голос, видеопоток и все вложения этим тестом не проверяются.</p>
    </div>
  </details>;
}
