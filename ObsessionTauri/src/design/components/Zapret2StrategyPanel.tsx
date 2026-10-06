import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { runtime, type Zapret2CatalogProfile, type Zapret2PickMode, type Zapret2PickerProgress, type Zapret2ScenarioProgress } from "../../lib/tauri";
import { useDpiStore } from "../../store/dpiStore";
import { useSettingsStore } from "../../store/settingsStore";
import { applicablePickerResult, scenarioBlocksApplication, useZapret2StrategyStore, zapret2ChoiceKey } from "../../store/zapret2StrategyStore";
import { Button, Chip, SectionLabel } from "./atoms";
import { DpiTestDetails } from "./DpiTestDetails";
import { Zapret2TournamentDetails } from "./Zapret2TournamentDetails";
import { Zapret2ScenarioActions, Zapret2ScenarioResults, Zapret2ScenarioTrial } from "./Zapret2ScenarioTrial";
import { Zapret2History } from "./Zapret2History";
import { Zapret2PickerAnimation } from "./Zapret2PickerAnimation";
import { Zapret2CardBackground } from "./Zapret2CardBackground";
import { Zapret2CompositionTrial } from "./Zapret2CompositionTrial";
import type { Zapret2PickerResult } from "../../lib/tauri";

const SERVICES = [
  { category: "discord", label: "Discord" },
  { category: "youtube_twitch", label: "YouTube" },
  { category: "gaming", label: "Gaming + GitHub" },
];
const MODES: { value: Zapret2PickMode; label: string; seconds: number; attempts: number }[] = [
  { value: "fast", label: "Быстро", seconds: 90, attempts: 3 },
  { value: "balanced", label: "Баланс", seconds: 180, attempts: 5 },
  { value: "deep", label: "Глубоко", seconds: 300, attempts: 7 },
];

function resultStatus(result: Zapret2PickerResult | undefined): string {
  if (!result) return "Не проверено";
  if (applicablePickerResult(result)) return result.transport === "quic" ? "QUIC подтверждён" : "Веб-проверки пройдены";
  if (result.status === "cancelled") return "Подбор отменён";
  if (result.status === "exhausted") return "Вариант не найден";
  if (result.status === "aborted") return "Подбор остановлен";
  return "Нужно перепроверить";
}

function ProfileRow({ category, label, transport, profiles, disabled, select }: {
  category: string; label: string; transport: string; profiles: Zapret2CatalogProfile[];
  disabled: boolean; select: (category: string, transport: string, profileId: string) => Promise<void>;
}) {
  const selected = profiles.find((profile) => profile.selected);
  const normal = profiles.filter((profile) => !profile.experimental);
  const experiments = profiles.filter((profile) => profile.experimental && !profile.candidate);
  const research = profiles.filter((profile) => profile.candidate);
  const title = transport === "voice" ? "Голос" : transport.toUpperCase();
  return <div className="flex flex-col gap-1">
    <div className="flex items-center justify-between gap-2 text-xs">
      <span className="font-medium text-ink-soft">{title}</span>
      <span className="text-ink-muted">{selected?.experimental ? "Эксперимент" : transport !== "tls" ? "Не проверен" : ""}</span>
    </div>
    <select aria-label={`${label} · ${title}`} value={selected?.profileId ?? ""}
      disabled={disabled || profiles.length === 0}
      onChange={(event) => void select(category, transport, event.target.value)}
      className="no-drag theme-morph control-anim w-full min-w-0 rounded-lg border border-white/10 bg-base-800/80 px-2.5 py-2 text-xs text-ink outline-none focus:border-accent/50 disabled:cursor-not-allowed disabled:opacity-50">
      {!selected && <option value="" disabled>{profiles.length ? "Выберите профиль" : "Нет профилей"}</option>}
      {normal.map((profile) => <option className="bg-base-800" key={profile.profileId} value={profile.profileId}>{profile.name}</option>)}
      {experiments.length > 0 && <optgroup label="Эксперимент">
        {experiments.map((profile) => <option className="bg-base-800" key={profile.profileId} value={profile.profileId}>{profile.name}</option>)}
      </optgroup>}
      {research.length > 0 && <optgroup label="Исследовательские профили">
        {research.map((profile) => <option className="bg-base-800" key={profile.profileId} value={profile.profileId}>{profile.name}</option>)}
      </optgroup>}
    </select>
    {selected?.candidate && <div className="text-xs text-ink-muted">
      <details className="mt-1">
        <summary className="cursor-pointer">Условия и источник</summary>
        <p className="mt-1">Исследовательский профиль · полное испытание сервиса не проведено.</p>
        <p className="my-1">{selected.candidate.notes}</p>
        <a href={selected.candidate.urls[0]} target="_blank" rel="noreferrer" className="text-accent underline">Источник профиля</a>
      </details>
    </div>}
  </div>;
}

export function Zapret2StrategyPanel() {
  const categories = useDpiStore((state) => state.selectedCategories);
  const transitioning = useDpiStore((state) => state.transitioning);
  const testing = useDpiStore((state) => state.testing);
  const settings = useSettingsStore(useShallow((state) => ({
    available: state.protectedDpiAvailable, mode: state.settings?.zapret2_pick_mode,
    saving: state.saving, error: state.error, patchConfirmed: state.patchConfirmed,
  })));
  const state = useZapret2StrategyStore();
  const [youtubeTransport, setYoutubeTransport] = useState<"tls" | "quic">("tls");
  const categorySignature = categories.join("|");
  const mode: Zapret2PickMode = MODES.some((item) => item.value === settings.mode)
    ? settings.mode as Zapret2PickMode : "balanced";
  const budget = MODES.find((item) => item.value === mode)!;
  const disabled = state.busy || state.loading || transitioning || testing || !settings.available || settings.saving;

  useEffect(() => { void state.load(categorySignature.split("|").filter(Boolean)); }, [categorySignature, state.load]);
  useEffect(() => { void state.loadHistory(); }, [state.loadHistory]);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void runtime.listen<Zapret2PickerProgress>("zapret2-picker-progress", (event) => state.applyProgress(event.payload))
      .then((cleanup) => { if (disposed) cleanup(); else unlisten = cleanup; })
      .catch((error) => useZapret2StrategyStore.setState({ error: String(error) }));
    return () => { disposed = true; unlisten?.(); };
  }, [state.applyProgress]);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void runtime.listen<Zapret2ScenarioProgress>("zapret2-scenario-progress", (event) => state.applyScenarioProgress(event.payload))
      .then((cleanup) => { if (disposed) cleanup(); else unlisten = cleanup; })
      .catch((error) => useZapret2StrategyStore.setState({ error: String(error) }));
    return () => { disposed = true; unlisten?.(); };
  }, [state.applyScenarioProgress]);

  return <div className="z2-strategy-panel flex flex-col gap-3">
    <SectionLabel>Автоподбор</SectionLabel>
    {state.scenarioProgress && <Zapret2ScenarioTrial progress={state.scenarioProgress} cancelling={state.cancelling}
      observing={state.observing} observe={(outcome) => void state.observeScenario(outcome)} cancel={() => void state.cancel()} />}
    {SERVICES.filter((service) => categories.includes(service.category)).map(({ category, label }) => {
      const entries = state.catalog.filter((profile) => profile.category === category);
      const transports = ["tls", "quic", ...(category === "discord" ? ["voice"] : [])];
      const progress = state.busy && state.operation === "pick" && state.progress?.category === category ? state.progress : null;
      const pickTransport = category === "youtube_twitch" ? youtubeTransport : "tls";
      const result = state.results[zapret2ChoiceKey(category, pickTransport)];
      const confirmed = applicablePickerResult(result);
      const results = ["tls", "quic"].map((transport) => state.results[zapret2ChoiceKey(category, transport)])
        .filter((value): value is Zapret2PickerResult => !!value);
      return <div key={category} className="z2-service-card" data-picking={!!progress} aria-label={"Конфигурации " + label}>
        <Zapret2CardBackground category={category} interactive={!state.busy} />
        <div className="z2-service-head">
          <div className="z2-service-title"><strong>{label}</strong>
            <span className="z2-service-status" data-confirmed={!progress && confirmed} role="status">
              {progress ? state.cancelling || progress.phase === "restoring" ? "Возвращаем…" : "Подбираем…"
                : state.loading && !entries.length ? "Загружаем…" : resultStatus(result)}
            </span>
          </div>
          <div className="z2-service-actions">
            <label className="z2-service-depth">Глубина
              <select className="no-drag" aria-label={`Глубина проверки для ${label}`} value={mode} disabled={disabled}
                onChange={(event) => void settings.patchConfirmed({ zapret2_pick_mode: event.target.value as Zapret2PickMode })}>
                {MODES.map((item) => <option className="bg-base-800" key={item.value} value={item.value}>{item.label}</option>)}
              </select>
            </label>
            <button type="button" className="z2-service-pick no-drag"
              aria-label={`${progress ? "Отмена и возврат" : "Подобрать"} ${label}`}
              disabled={progress ? state.cancelling || progress.phase === "restoring" : disabled || !entries.some((profile) => profile.transport === pickTransport && !profile.dataPlane)}
              onClick={() => progress ? void state.cancel() : void state.pick(category, pickTransport, mode)}>
              {progress ? state.cancelling ? "Возвращаем…" : "Отмена" : "Подобрать"}
            </button>
          </div>
        </div>
        <div className="z2-service-animation">
          <Zapret2PickerAnimation progress={progress} label={label} cancelling={state.cancelling}
            result={result} suppressExit={state.busy && !progress} />
        </div>
        {!progress && results.length > 0 && <div className="z2-service-outcomes">{results.map((entry) => {
          const passed = applicablePickerResult(entry);
          const picked = entries.find((profile) => profile.profileId === entry.profileId && profile.transport === entry.transport);
          const blocked = !!entry.profileId && scenarioBlocksApplication(state.scenarios ?? {}, entry, entry.profileId);
          return <div key={entry.transport} className="z2-service-outcome">
            {passed && <><Button variant="ghost" className="z2-apply" disabled={disabled || !picked || blocked}
              onClick={() => void state.applyResult(category, entry.transport)}>Применить {entry.transport.toUpperCase()}</Button>
              <p className="z2-service-result-note">{blocked ? "Испытание в приложении не пройдено. Повтори проверку."
                : entry.transport === "quic" ? "Подключение проверено. Видео — проверить вручную."
                : category === "discord" ? "Приложение и голос — проверить вручную."
                : category === "youtube_twitch" ? "Видео и вход — проверить вручную." : "Игры — проверить вручную."}</p></>}
            {entry.failure && <p className="text-danger" role="alert">{entry.failure}</p>}
          </div>;
        })}</div>}
        <details className="z2-service-details"><summary>Настройки и результаты</summary>
        <div className="z2-service-details-body">
        {category === "youtube_twitch" && <div className="flex gap-1.5" role="radiogroup" aria-label="Транспорт подбора YouTube">
          <Chip label="TLS" role="radio" ariaChecked={youtubeTransport === "tls"} active={youtubeTransport === "tls"}
            disabled={disabled} onClick={() => setYoutubeTransport("tls")} />
          <Chip label="QUIC" role="radio" ariaChecked={youtubeTransport === "quic"} active={youtubeTransport === "quic"}
            disabled={disabled} onClick={() => setYoutubeTransport("quic")} />
        </div>}
        {transports.map((transport) => {
          const profiles = entries.filter((profile) => profile.transport === transport && (transport === "voice" || !profile.dataPlane));
          if (profiles.length === 0) return null;
          return <ProfileRow key={transport} category={category} label={label} transport={transport}
            profiles={profiles} disabled={disabled} select={state.select} />;
        })}
        {category === "discord" && <Zapret2CompositionTrial profiles={entries.filter((entry)=>entry.transport==="tls")}
          disabled={disabled} test={(pair)=>void state.pick("discord","tls",mode,pair)} />}
        {state.loading && entries.length === 0 && <p className="text-xs text-ink-muted" role="status">Загружаем профили…</p>}
        {!progress && ["tls", "quic"].map((transport) => {
          const result = state.results[zapret2ChoiceKey(category, transport)];
          if (!result) return null;
          const passed = applicablePickerResult(result);
          const scenarios = state.scenarios ?? {};
          const scenarioBlocked = !!result.profileId && scenarioBlocksApplication(scenarios, result, result.profileId);
          const picked = entries.find((profile) => profile.profileId === result.profileId && profile.transport === transport);
          return <div key={transport} className="flex flex-col gap-2 text-xs">
            <span className={passed ? "text-ok" : "text-ink-muted"} role="status">
              {passed ? (transport === "quic" ? "QUIC-подключение подтверждено" : "Повторяемость проб подтверждена") + " · " + (picked?.name ?? result.profileId)
                : result.status === "cancelled" ? "Подбор отменён"
                : result.status === "tie" ? "Несколько профилей прошли пробы; преимущество по скорости не установлено"
                : result.status === "insufficient" ? "Есть успешные пробы; повторного подтверждения недостаточно"
                : result.status === "partial" ? "Есть частичные успехи; общий результат не подтверждён"
                : result.status === "inconclusive" ? "Не удалось достоверно сравнить профили"
                : result.status === "aborted" ? "Турнир остановлен: состояние runtime не подтверждено"
                : result.status === "exhausted" ? "Подтверждённый " + transport.toUpperCase() + "-профиль не найден" : "Все проверки не подтверждены"}
            </span>
            {scenarioBlocked && <p className="text-danger">Результат проверки в приложении не позволяет применить рекомендацию. Повтори испытание или подбор.</p>}
            <details>
              <summary className="cursor-pointer text-ink-soft">Подробности подбора</summary>
              <div className="mt-2 flex flex-col gap-2">
                {passed && transport === "quic" && <span className="text-ink-muted">Проверено подключение; воспроизведение видео — вручную.</span>}
                {passed && transport === "tls" && <span className="text-ink-muted">Проверены сетевые пробы и загрузка файлов. Запуск Discord и видеопоток — отдельные испытания.</span>}
                {result.historyWarning && <p className="text-ink-muted">{result.historyWarning}</p>}
                {passed && result.profileId && <><Zapret2ScenarioActions category={category} transport={transport} disabled={disabled}
                  test={(scenario) => void state.testScenario(category, transport, result.profileId!, scenario)} />
                  <Zapret2ScenarioResults results={Object.values(scenarios).filter((entry) => entry.runId === result.tournament?.runId
                    && entry.profileId === result.profileId)} /></>}
                <Zapret2TournamentDetails result={result} disabled={disabled}
                  scenarios={scenarios} test={(profileId, scenario) => void state.testScenario(category, transport, profileId, scenario)}
                  apply={(profileId) => void state.applyResult(category, transport, profileId)} />
                {result.report && <DpiTestDetails report={result.report} transport={transport} />}
              </div>
            </details>
          </div>;
        })}
        </div></details>
      </div>;
    })}
    <details className="text-xs text-ink-muted">
      <summary className="cursor-pointer">О подборе</summary>
      <p className="mt-2">До {budget.seconds} с и {budget.attempts} замеров. После каждого замера возвращается исходный профиль.</p>
      <p className="mt-1">Для дополняющих профилей Discord — до 3 дополнительных замеров в том же лимите времени.</p>
      <p className="mt-1">Первичный отбор — до 3 с сетевых проб на профиль. Успешные варианты проходят отдельную перепроверку.</p>
    </details>
    <Zapret2History records={state.history ?? []} />
    {state.historyError && <p className="text-xs text-ink-muted">{state.historyError}</p>}
    {(state.error || settings.error) && <p className="text-xs text-danger" role="alert">{state.error || settings.error}</p>}
  </div>;
}
