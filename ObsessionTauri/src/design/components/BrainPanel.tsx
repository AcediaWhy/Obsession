import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { api, on, type BrainStatus } from "../../lib/tauri";
import { SectionLabel, Switch } from "./atoms";

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
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<BrainStatus | null>(null);

  useEffect(() => {
    // Начальное состояние: флаг из настроек + текущий статус, если Мозг жив.
    api.getSettings().then((s) => setEnabled(s.auto_recovery)).catch(() => {});
    api.brainGetStatus().then((s) => s && setStatus(s)).catch(() => {});
    let unlisten: (() => void) | undefined;
    on.brainStatus((s) => setStatus(s)).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const toggle = async (next: boolean) => {
    setBusy(true);
    try {
      await api.brainSetEnabled(next);
      setEnabled(next);
      if (!next) setStatus(null);
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

      <AnimatePresence initial={false}>
        {enabled && status && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ type: "spring", stiffness: 320, damping: 30 }}
            className="flex flex-col gap-1 overflow-hidden rounded-lg bg-white/5 px-3 py-2 text-sm"
          >
            <div className="flex items-center justify-between">
              <span className="text-ink-muted">Состояние</span>
              <span className={`font-semibold ${PHASE_TONE[status.phase]}`}>
                {PHASE_LABEL[status.phase]}
                {status.ladderLevel !== "none" && (
                  <span className="ml-1 text-[11px] text-ink-muted">
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
                <span className="text-[11px] text-ink-muted">{status.asnRegion}</span>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {enabled && !status && (
        <p className="px-1 text-[11px] text-ink-muted">
          Запустите обход — Мозг подключится к сессии.
        </p>
      )}
    </div>
  );
}
