import { useEffect, useState } from "react";

import { api, type BrainStatus } from "../../lib/tauri";
import { useBrainStore } from "../../store/brainStore";
import { useSettingsStore } from "../../store/settingsStore";
import { SectionLabel, Switch } from "./atoms";
import { Collapse } from "./Collapse";

// Debug-читалка контура надёжности: тумблер авто-восстановления (Мозг L3) +
// живой агрегированный статус машины состояний (`brain://status`). Намеренно
// компактна — глубокий UI появится после обкатки протокола охлаждения.

const PHASE_LABEL: Record<BrainStatus["phase"], string> = {
  idle: "Ожидание",
  confirming: "Подтверждение",
  healthy: "Здоров",
  suspect: "Подозрение",
  switching: "Переключение",
  frozen: "Охлаждение",
  exhausted: "Исчерпано",
};

const PHASE_TONE: Record<BrainStatus["phase"], string> = {
  idle: "text-ink-muted",
  confirming: "text-warn",
  healthy: "text-ok",
  suspect: "text-warn",
  switching: "text-warn",
  frozen: "text-danger",
  exhausted: "text-danger",
};

function frozenRemaining(status: BrainStatus): string | null {
  if (status.phase !== "frozen" || status.frozenUntilMs == null) return null;
  // frozenUntilMs — в монотонике рантайма (не Date), поэтому показываем полный
  // backoff, а не разницу с Date.now() (эпохи разные).
  if (status.backoffSecs == null) return "пауза";
  const m = Math.floor(status.backoffSecs / 60);
  const s = status.backoffSecs % 60;
  return m > 0 ? `${m} мин ${s} с` : `${s} с`;
}

export function BrainPanel() {
  const configuredEnabled = useSettingsStore(
    (state) => state.settings?.auto_recovery ?? false,
  );
  const [enabled, setEnabled] = useState(configuredEnabled);
  const [busy, setBusy] = useState(false);
  const status = useBrainStore((state) => state.status);

  useEffect(() => {
    setEnabled(configuredEnabled);
  }, [configuredEnabled]);

  const toggle = async (next: boolean) => {
    setBusy(true);
    try {
      await api.brainSetEnabled(next);
      setEnabled(next);
      useSettingsStore
        .getState()
        .applyLocalPatch({ auto_recovery: next });
      if (!next) useBrainStore.getState().clearLocalStatus();
    } catch {
      // откат визуального состояния при ошибке
    } finally {
      setBusy(false);
    }
  };

  const remaining = status && frozenRemaining(status);

  return (
    <div className="w-full">
      <div className="mb-2 flex items-center justify-between">
        <SectionLabel>Авто-восстановление</SectionLabel>
        <Switch checked={enabled} onChange={toggle} disabled={busy} />
      </div>

      <Collapse
        open={Boolean(enabled && status)}
        className="rounded-lg bg-white/5"
      >
        {status ? (
          <div className="flex flex-col gap-1 px-3 py-2 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-ink-muted">Состояние</span>
              <span className={`font-semibold ${PHASE_TONE[status.phase]}`}>
                {PHASE_LABEL[status.phase]}
                {status.ladderLevel !== "none" && (
                  <span className="ml-1 text-2xs tabular-nums text-ink-muted">
                    · {status.ladderLevel.toUpperCase()}
                  </span>
                )}
              </span>
            </div>
            {status.currentConf && (
              <div className="flex items-center justify-between">
                <span className="text-ink-muted">Стратегия</span>
                <span className="text-ink-soft">{status.currentConf}</span>
              </div>
            )}
            {remaining && (
              <div className="flex items-center justify-between">
                <span className="text-ink-muted">Пауза</span>
                <span className="text-danger">{remaining}</span>
              </div>
            )}
            {status.asnRegion && (
              <div className="flex items-center justify-between">
                <span className="text-ink-muted">Сеть</span>
                <span className="text-2xs tabular-nums text-ink-muted">{status.asnRegion}</span>
              </div>
            )}
          </div>
        ) : null}
      </Collapse>

      {enabled && !status && (
        <p className="px-1 text-2xs text-ink-muted">
          Запустите обход — Мозг подключится к сессии.
        </p>
      )}
    </div>
  );
}
