import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { runtime, type Zapret2CatalogProfile, type Zapret2PickMode, type Zapret2PickerProgress, type Zapret2ScenarioProgress } from "../../lib/tauri";
import { useDpiStore } from "../../store/dpiStore";
import { useSettingsStore } from "../../store/settingsStore";
import { applicablePickerResult, confirmedPickerCandidate, historicalProfileCheck, scenarioBlocksApplication, useZapret2StrategyStore, zapret2ChoiceKey } from "../../store/zapret2StrategyStore";
import { Button, Chip, SectionLabel } from "./atoms";
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
const MODES: { value: Zapret2PickMode; label: string }[] = [
  { value: "fast", label: "Быстро" },
  { value: "balanced", label: "Баланс" },
  { value: "deep", label: "Глубоко" },
];

function resultStatus(result: Zapret2PickerResult | undefined): string {
  if (!result) return "Готов к подбору";
  if (applicablePickerResult(result)) return result.transport === "quic" ? "Подключение проверено" : "Проверки пройдены";
  if (result.status === "cancelled") return "Подбор отменён";
  if (result.status === "exhausted") return "Вариант не найден";
  if (result.status === "aborted") return "Подбор остановлен";
  if (result.status === "tie") return "Несколько подходящих вариантов";
  return "Нужно перепроверить";
}

function recommendedProfile(result: Zapret2PickerResult | undefined, profiles: Zapret2CatalogProfile[]) {
  if (applicablePickerResult(result)) return result!.profileId;
  if (result?.status !== "tie") return;
  const selected = profiles.find((profile) => profile.transport === result.transport && profile.selected);
  if (selected && confirmedPickerCandidate(result, selected.profileId)) return selected.profileId;
  return result.tournament?.candidates.find((candidate) => confirmedPickerCandidate(result, candidate.profileId))?.profileId;
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
      <span className="text-ink-muted">{selected?.experimental ? "Эксперимент" : selected ? "Выбран" : ""}</span>
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
      const recommendation = recommendedProfile(result, entries);
      const confirmed = !!recommendation;
      const picked = entries.find((profile) => profile.profileId === recommendation && profile.transport === pickTransport);
      const blocked = !!result && !!recommendation && scenarioBlocksApplication(state.scenarios ?? {}, result, recommendation);
      const selected = entries.find((profile) => profile.selected && profile.transport === pickTransport);
      const archived = historicalProfileCheck(state.history ?? [], selected)
        ?? (category === "youtube_twitch" ? historicalProfileCheck(state.history ?? [], entries.find((profile) => profile.selected && profile.transport === "quic")) : undefined);
      const applied = selected && state.applied?.[zapret2ChoiceKey(category, pickTransport)] === selected.profileId;
      const status = result ? confirmed && result.status === "tie" ? "Вариант найден" : resultStatus(result)
        : applied ? "Настройки применены" : archived ? archived.passed
          ? archived.transport === "quic" ? "Подключение ранее проверено" : "Ранее проверено" : "Нужно перепроверить"
          : selected ? "Настроено" : resultStatus(undefined);
      const results = ["tls", "quic"].map((transport) => state.results[zapret2ChoiceKey(category, transport)])
        .filter((value): value is Zapret2PickerResult => !!value);
      return <div key={category} className="z2-service-card" data-picking={!!progress} aria-label={"Конфигурации " + label}>
        <Zapret2CardBackground category={category} interactive={!state.busy} />
        <div className="z2-service-head">
          <div className="z2-service-title"><strong>{label}</strong>
            <span className="z2-service-status" data-confirmed={!progress && (confirmed || applied)} role="status"
              title={!result && archived ? `Сетевые проверки: ${new Date(archived.checkedAt).toLocaleString()}. Это сохранённый результат, не новая проверка.` : undefined}>
              {progress ? state.cancelling || progress.phase === "restoring" ? "Возвращаем…" : "Подбираем…"
                : state.loading && !entries.length ? "Загружаем…" : status}
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
        {!progress && result && <div className="z2-service-outcomes">
          <div className="z2-service-outcome">
            {confirmed && <><Button variant="ghost" className="z2-apply" aria-label={`Применить настройки ${label}`}
              disabled={disabled || !picked || blocked}
              onClick={() => void state.applyResult(category, pickTransport, recommendation)}>Применить</Button>
              <p className="z2-service-result-note">{blocked ? "Испытание в приложении не пройдено. Повтори проверку."
                : pickTransport === "quic" ? "Видео проверь в YouTube."
                : category === "discord" ? "Запуск приложения и голос проверь в Discord."
                : category === "youtube_twitch" ? "Видео и вход проверь в YouTube." : "Игры проверь после применения."}</p></>}
            {result.failure && <p className="text-danger" role="alert">{result.failure}</p>}
          </div>
        </div>}
        {!progress && results.length > 0 && <details className="z2-service-details z2-service-results">
          <summary>Результаты проверки</summary>
          <div className="z2-service-details-body">
          {results.map((result) => {
          const transport = result.transport;
          const recommendation = recommendedProfile(result, entries);
          const passed = !!recommendation;
          const scenarios = state.scenarios ?? {};
          const scenarioBlocked = !!recommendation && scenarioBlocksApplication(scenarios, result, recommendation);
          const picked = entries.find((profile) => profile.profileId === recommendation && profile.transport === transport);
          return <div key={transport} className="flex flex-col gap-2 text-xs">
            {results.length > 1 && <strong className="text-ink-soft">{transport === "quic" ? "Альтернативное подключение" : "Основное подключение"}</strong>}
            <span className={passed ? "text-ok" : "text-ink-muted"} role="status">
              {passed ? transport === "quic" ? "Подключение проверено" : "Сетевые проверки пройдены" : resultStatus(result)}
            </span>
            {result.status === "tie" && passed && <p className="text-ink-muted">Подходят несколько вариантов. Можно применить предложенный.</p>}
            {result.failure && transport !== pickTransport && <p className="text-danger" role="alert">{result.failure}</p>}
            {passed && transport !== pickTransport && <Button variant="ghost" disabled={disabled || !picked || scenarioBlocked}
              onClick={() => void state.applyResult(category, transport, recommendation)}>Применить этот вариант</Button>}
            {scenarioBlocked && <p className="text-danger">Результат проверки в приложении не позволяет применить рекомендацию. Повтори испытание или подбор.</p>}
            {passed && recommendation && <><Zapret2ScenarioActions category={category} transport={transport} disabled={disabled}
              test={(scenario) => void state.testScenario(category, transport, recommendation, scenario)} />
              <Zapret2ScenarioResults results={Object.values(scenarios).filter((entry) => entry.runId === result.tournament?.runId
                && entry.profileId === recommendation)} /></>}
            {passed && transport === "quic" && <p className="text-ink-muted">Соединение установлено. Видео проверяется отдельно.</p>}
            {result.historyWarning && <p className="text-ink-muted">{result.historyWarning}</p>}
          </div>;
          })}
          </div>
        </details>}
        <details className="z2-service-details z2-manual-settings"><summary>Ручная настройка</summary>
        <div className="z2-service-details-body">
          <p className="text-ink-muted">Автоподбор сам выбирает профили. Здесь их можно изменить вручную.</p>
          {category === "youtube_twitch" && <div className="flex gap-1.5" role="radiogroup" aria-label="Транспорт подбора YouTube">
            <Chip label="TLS" role="radio" ariaChecked={youtubeTransport === "tls"} active={youtubeTransport === "tls"}
              disabled={disabled} onClick={() => setYoutubeTransport("tls")} />
            <Chip label="QUIC" role="radio" ariaChecked={youtubeTransport === "quic"} active={youtubeTransport === "quic"}
              disabled={disabled} onClick={() => setYoutubeTransport("quic")} />
          </div>}
          <div className="grid gap-3" aria-label={`Текущая конфигурация ${label}`}>
            <strong className="text-xs font-medium text-ink-soft">Текущая конфигурация</strong>
            {transports.map((transport) => {
              const profiles = entries.filter((profile) => profile.transport === transport && (transport === "voice" || !profile.dataPlane));
              if (profiles.length === 0) return null;
              return <ProfileRow key={transport} category={category} label={label} transport={transport}
                profiles={profiles} disabled={disabled} select={state.select} />;
            })}
          </div>
          {category === "discord" && <Zapret2CompositionTrial profiles={entries.filter((entry)=>entry.transport==="tls")}
            disabled={disabled} test={(pair)=>void state.pick("discord","tls",mode,pair)} />}
          {state.loading && entries.length === 0 && <p className="text-xs text-ink-muted" role="status">Загружаем профили…</p>}
        </div></details>
      </div>;
    })}
    <Zapret2History records={state.history ?? []} />
    {state.historyError && <p className="text-xs text-ink-muted">{state.historyError}</p>}
    {(state.error || settings.error) && <p className="text-xs text-danger" role="alert">{state.error || settings.error}</p>}
  </div>;
}
