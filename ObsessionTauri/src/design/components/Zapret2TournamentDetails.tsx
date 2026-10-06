import type { Zapret2PickerResult, Zapret2Scenario, Zapret2ScenarioResult } from "../../lib/tauri";
import { confirmedPickerCandidate, scenarioBlocksApplication } from "../../store/zapret2StrategyStore";
import { Button } from "./atoms";
import { Zapret2ScenarioActions, Zapret2ScenarioResults } from "./Zapret2ScenarioTrial";

const outcomes: Record<string, string> = {
  passed: "Пройдено", failed: "Отказ", inconclusive: "Не удалось оценить", not_tested: "Не проверено", cancelled: "Отменено",
};
const contexts: Record<string, string> = {
  network_changed: "Сеть изменилась — раунд не участвует в сравнении.",
  network_unknown: "Сопоставимость сети не подтверждена — раунд не участвует в сравнении.",
};
const stages: Record<string, string> = { tls: "ожидание TLS", http_headers: "ожидание HTTP", http_body: "чтение ответа", complete: "завершено" };

export function Zapret2TournamentDetails({ result, disabled, apply, scenarios, test }: {
  result: Zapret2PickerResult; disabled: boolean; apply: (profileId: string) => void;
  scenarios: Record<string, Zapret2ScenarioResult>; test: (profileId: string, scenario: Zapret2Scenario) => void;
}) {
  const tournament = result.tournament;
  if (!tournament) return null;
  return <div className="flex flex-col gap-2 text-xs">
    <p className="text-ink-muted">Замеров: {result.checked} / {tournament.maxAttempts} · для подтверждения: {tournament.requiredConfirmations}.
      {" "}Прочитано {(tournament.bytesRead / 1024).toFixed(0)} КБ · {(tournament.elapsedMs / 1000).toFixed(1)} с.</p>
    {tournament.stopReason === "budget" && <p className="text-ink-muted">Бюджет завершён; неподтверждённые результаты сохранены.</p>}
    <p className="text-ink-muted">Воспроизведение видео, вход в аккаунт и голос требуют отдельных испытаний.</p>
    <p className="text-ink-muted">* Обязательные пробы. Остальные строки помогают уточнить результат.</p>
    <details>
      <summary className="cursor-pointer font-medium text-ink-soft">Результаты по профилям и функциям</summary>
      <div className="mt-2 flex flex-col gap-3">
        {tournament.candidates.map((candidate) => <div key={candidate.profileId} className="rounded-lg border border-white/10 p-2">
          <p className="font-medium text-ink-soft">{candidate.label}</p>
          {candidate.composition && <p className="my-1 text-ink-muted">API/Gateway: {tournament.candidates.find((entry) => entry.profileId === candidate.composition?.apiProfileId)?.label ?? candidate.composition.apiProfileId}
            {" · "}CDN: {tournament.candidates.find((entry) => entry.profileId === candidate.composition?.cdnProfileId)?.label ?? candidate.composition.cdnProfileId}.
            {" "}Полные замеры ниже выполнены на самой сборке.</p>}
          <p className="mb-2 text-ink-muted">Полные замеры: {candidate.completeRounds} / {candidate.rounds}
            {candidate.status === "confirmed" ? " · повторяемость подтверждена" : candidate.status === "not_tested" ? " · не проверен" : " · подтверждения недостаточно"}</p>
          {result.transport === "tls" && <p className="mb-2 text-ink-muted">Обработка нужными блоками winws2:
            {" "}{candidate.routingVerifiedRounds != null ? `${candidate.routingVerifiedRounds} / ${candidate.rounds}` : "в этой версии теста не проверялась"}.
            {" "}Это подтверждение выполнения правил; доставка изменённых пакетов отдельно не измерена.</p>}
          {candidate.transferBytesPerSec != null && <p className="mb-2 text-ink-soft">Загрузка тестового файла: {(candidate.transferBytesPerSec / 1024).toFixed(0)} КБ/с.
            {" "}Это скорость публичного файла; скорость видеопотока и запуск клиента проверяются отдельно.</p>}
          {candidate.functions.length > 0 && <table className="w-full text-left" aria-label={`Функции · ${candidate.label}`}>
            <thead className="text-ink-muted"><tr><th className="font-normal">Функция</th><th className="font-normal">Успех</th><th className="font-normal">Отказ</th><th className="font-normal">Не оценено</th></tr></thead>
            <tbody>{candidate.functions.map((entry) => <tr key={entry.function}>
              <td className="py-1 pr-2">{entry.label}{entry.required ? " *" : ""}{entry.notTested === candidate.rounds ? " · не проверено" : ""}</td>
              <td className={entry.passed > 0 ? "text-ok" : "text-ink-muted"}>{entry.passed}</td>
              <td className={entry.failed > 0 ? "text-danger" : "text-ink-muted"}>{entry.failed}</td>
              <td className="text-ink-muted">{entry.inconclusive + entry.cancelled + entry.notTested}</td>
            </tr>)}</tbody>
          </table>}
          {confirmedPickerCandidate(result, candidate.profileId) && candidate.profileId !== result.profileId && <>
            <Button variant="ghost" disabled={disabled || scenarioBlocksApplication(scenarios, result, candidate.profileId)} onClick={() => apply(candidate.profileId)}>Применить этот профиль</Button>
            <Zapret2ScenarioActions category={result.category} transport={result.transport} disabled={disabled}
              test={(scenario) => test(candidate.profileId, scenario)} />
          </>}
          {candidate.profileId !== result.profileId && <Zapret2ScenarioResults results={Object.values(scenarios)
            .filter((entry) => entry.runId === tournament.runId && entry.profileId === candidate.profileId)} />}
        </div>)}
      </div>
    </details>
    <details>
      <summary className="cursor-pointer text-ink-soft">Подробности раундов</summary>
      {tournament.rounds.map((round) => <div key={round.ordinal} className="mt-2 rounded-lg border border-white/10 p-2">
        <p className="text-ink-soft">{round.ordinal}. {round.phase === "composition" ? "Проверка сборки" : round.phase === "confirmation" ? "Повторная проверка" : "Отбор"} · {tournament.candidates.find((c) => c.profileId === round.profileId)?.label ?? round.profileId}</p>
        {!round.comparable && <p className="text-ink-muted">{contexts[round.contextReason ?? ""] ?? "Раунд не участвует в сравнении."}</p>}
        {round.packetCapture && (round.packetCapture.state !== "active" || round.packetCapture.sensorFailed
          || round.packetCapture.queueDrops > 0 || round.packetCapture.parseErrors > 0 || round.packetCapture.capacityEvictions > 0)
          && <p className="text-ink-muted">Наблюдение TCP неполное: {round.packetCapture.startupError ?? round.packetCapture.state}. Отсутствие записи не доказывает отсутствие пакета.</p>}
        {round.functions.map((entry) => <div className="mt-1 text-ink-muted" key={entry.function}>
          <p>{entry.label}: {outcomes[entry.outcome]}{entry.reason ? ` · ${entry.reason}` : ""}</p>
          {entry.connections?.map((connection, index) => {
            const flow = round.packetCapture?.flows.find((flow) => flow.localAddress === connection.localAddress && flow.remoteAddress === connection.remoteAddress);
            return <div key={`${connection.localAddress}-${index}`}><p>
            {connection.purpose}: {connection.status === "verified" ? `блок ${connection.expectedProfileId} подтверждён`
              : connection.status === "wrong_profile" ? `сработал другой блок: ${connection.observedProfiles.join(", ")}`
              : connection.status === "action_error" ? "ошибка выполнения правила" : "обработка нужным блоком не подтверждена"}.
            </p>
            {connection.http && <p>{stages[connection.http.stage] ?? connection.http.stage} · {connection.http.elapsedMs} мс ·
              {" "}{connection.http.httpStatus == null ? "HTTP-ответ не получен" : `HTTP ${connection.http.httpStatus}`} · {connection.http.bytes} Б.
              {connection.http.tlsMs != null ? ` TLS: ${connection.http.tlsMs} мс.` : ""}</p>}
            {flow && <p>TCP ACK: {flow.ackedClientBytes ?? "не наблюдался"} · ответные данные: {flow.inboundPayloadBytes} Б ·
              {" "}ServerHello: {flow.tlsServerHelloObserved ? "наблюдался" : "не наблюдался"}
              {flow.inboundReset ? " · reset" : ""}{flow.tlsAlertObserved ? " · TLS alert" : ""}.</p>}
            </div>;
          })}
          {entry.tlsHandshakes?.map((hello) => <p key={hello.kind}>
            {hello.kind === "large_early_sni" ? "Ранний SNI" : "Поздний SNI"} · {hello.handshakeBytes} байт ·
            {" "}{hello.authenticatedTls ? "TLS подтверждён" : "TLS не подтверждён"}
            {hello.applicationOk ? " · ответ проверен" : ""}
          </p>)}
        </div>)}
      </div>)}
    </details>
  </div>;
}
