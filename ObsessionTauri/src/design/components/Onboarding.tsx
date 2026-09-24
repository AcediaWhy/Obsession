import { useState } from "react";

import type { OnboardingDestination } from "../../lib/onboarding";
import { useOnboardingStore } from "../../store/onboardingStore";
import { useSettingsStore } from "../../store/settingsStore";
import { launcherBootstrap } from "../../store/launcherBootstrap";
import { Button } from "./atoms";

// Совместимость с незавершёнными операциями старого мастера настройки.
export function Onboarding({
  onDestination,
}: {
  onDestination?: (destination: OnboardingDestination) => void;
}) {
  const snapshot = useOnboardingStore((s) => s.snapshot);
  const busy = useOnboardingStore((s) => s.busy);
  const failure = useOnboardingStore((s) => s.failure);
  const refresh = useOnboardingStore((s) => s.refresh);
  const verify = useOnboardingStore((s) => s.verify);
  const rollback = useOnboardingStore((s) => s.rollback);
  const complete = useOnboardingStore((s) => s.complete);
  const launchRepair = useOnboardingStore((s) => s.launchRepair);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const serviceAvailable = useSettingsStore((s) => s.protectedRuntime.serviceAvailable);
  const [confirmRollback, setConfirmRollback] = useState(false);
  const [repairLaunched, setRepairLaunched] = useState(false);

  const transaction = snapshot?.terminalStatus === "active" ? snapshot.transaction : null;
  if (!settingsLoaded || (!transaction && !failure && serviceAvailable)) return null;

  const rolledBack = transaction?.status === "rolled_back";
  const applied = transaction?.status === "applied";
  const verification = transaction?.verification;
  const needsRepair = !serviceAvailable || failure?.code === "RUNTIME_UNAVAILABLE";
  const title = transaction
    ? rolledBack ? "Прежние настройки восстановлены" : "Осталась незавершённая настройка"
    : needsRepair ? "Служба Obsession недоступна" : "Не удалось прочитать сохранённую настройку";

  const finish = async () => {
    if (await complete("overview")) onDestination?.("overview");
  };
  const undo = async () => {
    if (await rollback()) {
      setConfirmRollback(false);
      onDestination?.("overview");
    }
  };

  return (
    <section aria-label="Восстановление настройки" aria-busy={busy}
      className="no-drag max-h-[45%] shrink-0 overflow-y-auto rounded-xl border border-warn/30 bg-base-900/95 p-4 text-ink">
      <h2 className="text-sm font-semibold">{title}</h2>
      <p className="mt-1 text-xs leading-5 text-ink-soft">
        {transaction
          ? rolledBack
            ? "Изменения прежнего мастера отменены. Можно закрыть уведомление."
            : applied
              ? "Прежний мастер применил настройки, но не завершил работу. Проверьте результат и выберите, сохранить его или вернуть предыдущие настройки."
              : "Прежний мастер был прерван. Можно повторить откат сохранённых изменений."
          : needsRepair
            ? "Для работы обхода нужна системная служба. Восстановите её через установщик и повторите проверку."
            : "Повторите проверку. Сохранённые данные не удалены."}
      </p>
      {applied && verification && (
        <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs" aria-label="Результаты проверки">
          {verification.targets.filter((target) => target.status !== "not_selected").map((target) => (
            <li key={target.id}>{target.label}: {target.status === "passed" ? "отвечает" : target.status === "failed" ? "проверка не пройдена" : "не удалось проверить"}</li>
          ))}
        </ul>
      )}
      {failure && (
        <p role="alert" className="mt-2 text-xs text-warn">
          {failure.code === "REPAIR_UNAVAILABLE"
            ? "Не удалось открыть восстановление. Запустите установщик Obsession вручную."
            : failure.code === "ROLLBACK_FAILED"
              ? "Откат не завершён. Сохранённые шаги доступны для повторной попытки."
              : "Действие не удалось завершить. Повторите проверку или попробуйте ещё раз."}
        </p>
      )}
      {repairLaunched && <p role="status" className="mt-2 text-xs text-ink-soft">После завершения установщика нажмите «Проверить снова».</p>}
      {busy && <p role="status" className="mt-2 text-xs text-ink-soft">Выполняется операция…</p>}
      {confirmRollback ? (
        <div className="mt-3">
          <p className="text-xs text-warn">Вернуть настройки, сохранённые до запуска прежнего мастера? Это может заменить изменения, сделанные позже, и остановить запущенные им функции.</p>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button disabled={busy} variant="danger" onClick={() => void undo()}>Вернуть прежние настройки</Button>
            <Button disabled={busy} variant="ghost" onClick={() => setConfirmRollback(false)}>Отмена</Button>
          </div>
        </div>
      ) : (
        <div className="mt-3 flex flex-wrap gap-2">
          {rolledBack && <Button disabled={busy} onClick={() => void finish()}>Закрыть уведомление</Button>}
          {applied && verification && <Button disabled={busy} onClick={() => void finish()}>Оставить применённые настройки</Button>}
          {applied && <Button disabled={busy || !serviceAvailable} variant="ghost" onClick={() => void verify()}>Проверить настройки</Button>}
          {transaction && !rolledBack && <Button disabled={busy || !serviceAvailable} variant="ghost" onClick={() => setConfirmRollback(true)}>Отменить прежнюю настройку</Button>}
          {needsRepair && <Button disabled={busy} variant="ghost" onClick={() => void launchRepair().then((ok) => { if (ok) setRepairLaunched(true); })}>Восстановить службу</Button>}
          <Button disabled={busy} variant="ghost" onClick={() => { void refresh(); void launcherBootstrap.refresh(); }}>Проверить снова</Button>
        </div>
      )}
    </section>
  );
}
