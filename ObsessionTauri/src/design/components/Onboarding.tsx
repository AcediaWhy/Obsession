import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { motion } from "framer-motion";

import {
  type OnboardingDestination,
  type OnboardingDraft,
  type OnboardingGoals,
  type VerificationOutcome,
} from "../../lib/onboarding";
import { useOnboardingStore } from "../../store/onboardingStore";
import { useThemeStore, type Theme } from "../../store/themeStore";
import { Button } from "./atoms";
import { spring } from "../tokens";

const PANEL_WIDTH = "min(880px, 94vw)";
const BODY_MAX_HEIGHT = "min(58vh, 390px)";

export function getOnboardingKeyboardIntent(
  key: string,
  confirmationOpen: boolean,
  saving: boolean,
): "open_confirmation" | "close_confirmation" | null {
  if (key !== "Escape" || saving) return null;
  return confirmationOpen ? "close_confirmation" : "open_confirmation";
}

export function getOnboardingPrimaryButtonClass(theme: Theme): string {
  return [
    "min-h-11 hover:!bg-accent/90 focus-visible:!ring-accent-cyan",
    theme === "aurora" || theme === "fallendown" ? "!text-white" : "!text-[#05060B]",
  ].join(" ");
}

function StatusRow({ label, ok, detail }: { label: string; ok: boolean; detail: string }) {
  return (
    <div className="flex items-start gap-3 rounded-xl border border-white/[0.07] bg-base-800/45 px-3.5 py-3">
      <span
        className={`mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full ${
          ok ? "bg-ok/15 text-ok" : "bg-warn/15 text-warn"
        }`}
        aria-hidden="true"
      >
        {ok ? "✓" : "!"}
      </span>
      <span className="min-w-0">
        <strong className="block text-sm font-semibold text-ink">{label}</strong>
        <small className="mt-0.5 block text-xs leading-5 text-ink-soft">{detail}</small>
      </span>
    </div>
  );
}

function GoalCard({
  id,
  title,
  description,
  checked,
  onChange,
}: {
  id: keyof OnboardingGoals;
  title: string;
  description: string;
  checked: boolean;
  onChange: (id: keyof OnboardingGoals) => void;
}) {
  return (
    <label
      className={`no-drag flex cursor-pointer gap-3 rounded-xl border p-4 transition-colors ${
        checked ? "border-accent/55 bg-accent/10" : "border-white/[0.08] bg-base-800/45"
      }`}
    >
      <input
        type="checkbox"
        checked={checked}
        onChange={() => onChange(id)}
        className="mt-1 h-4 w-4 accent-[rgb(var(--accent))]"
      />
      <span>
        <strong className="block text-sm font-semibold text-ink">{title}</strong>
        <small className="mt-1 block text-xs leading-5 text-ink-soft">{description}</small>
      </span>
    </label>
  );
}

function outcomeCopy(outcome: VerificationOutcome | undefined, rolledBack = false) {
  if (rolledBack) {
    return [
      "Изменения безопасно отменены",
      "Мастер восстановил предыдущую конфигурацию. Приложение продолжает работать без изменений.",
    ];
  }
  switch (outcome) {
    case "success":
      return ["Настройка подтверждена", "Все выбранные цели прошли адресную проверку."];
    case "partial":
      return ["Часть целей работает", "Рабочие функции можно оставить, а непройденные проверить повторно."];
    case "failed":
      return ["Проверка не пройдена", "Конфигурация применена, но выбранные цели не подтвердили работу."];
    default:
      return [
        "Автопроверка не дала однозначного ответа",
        "Настройки применены, но адресные HTTPS-проверки не получили ответ. Проверь выбранные сервисы или повтори попытку.",
      ];
  }
}

function destinationFor(draft: OnboardingDraft): OnboardingDestination {
  const selected = Object.entries(draft.goals).filter(([, enabled]) => enabled);
  if (selected.length !== 1) return "overview";
  return selected[0][0] as OnboardingDestination;
}

export function Onboarding({
  onDestination,
}: {
  onDestination?: (destination: OnboardingDestination) => void;
}) {
  const theme = useThemeStore((state) => state.theme);
  const snapshot = useOnboardingStore((state) => state.snapshot);
  const readiness = useOnboardingStore((state) => state.readiness);
  const busy = useOnboardingStore((state) => state.busy);
  const failure = useOnboardingStore((state) => state.failure);
  const checkReadiness = useOnboardingStore((state) => state.checkReadiness);
  const saveDraft = useOnboardingStore((state) => state.saveDraft);
  const buildPlan = useOnboardingStore((state) => state.buildPlan);
  const apply = useOnboardingStore((state) => state.apply);
  const verify = useOnboardingStore((state) => state.verify);
  const acceptVerification = useOnboardingStore((state) => state.acceptVerification);
  const rollback = useOnboardingStore((state) => state.rollback);
  const complete = useOnboardingStore((state) => state.complete);
  const skip = useOnboardingStore((state) => state.skip);
  const cancel = useOnboardingStore((state) => state.cancel);
  const launchRepair = useOnboardingStore((state) => state.launchRepair);
  const clearFailure = useOnboardingStore((state) => state.clearFailure);

  const [draft, setDraft] = useState<OnboardingDraft | null>(snapshot?.draft ?? null);
  const [localView, setLocalView] = useState<"welcome" | "readiness" | "goals" | null>(
    snapshot?.phase === "welcome" ? "welcome" : null,
  );
  const dialogRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const titleId = useId();
  const descriptionId = useId();
  const primaryButtonClass = getOnboardingPrimaryButtonClass(theme);

  useEffect(() => {
    if (snapshot?.draft) setDraft(snapshot.draft);
  }, [snapshot?.revision]);

  useEffect(() => {
    if (
      snapshot?.phase === "recovery_required" &&
      readiness === null &&
      !busy &&
      !failure
    ) {
      void checkReadiness();
    }
  }, [busy, checkReadiness, failure, readiness, snapshot?.phase]);

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, [snapshot?.phase, localView]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Tab") {
        const focusable = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])',
          ) ?? [],
        );
        if (!focusable.length) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
      if (
        event.key === "Escape" &&
        !busy &&
        !snapshot?.transaction &&
        snapshot?.presentation === "modal"
      ) {
        event.preventDefault();
        void cancel();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [busy, cancel, snapshot?.presentation, snapshot?.transaction]);

  const toggleGoal = useCallback(
    (id: keyof OnboardingGoals) => {
      setDraft((current) =>
        current
          ? { ...current, goals: { ...current.goals, [id]: !current.goals[id] } }
          : current,
      );
      clearFailure();
    },
    [clearFailure],
  );

  const selectedCount = useMemo(
    () => (draft ? Object.values(draft.goals).filter(Boolean).length : 0),
    [draft],
  );

  if (!snapshot || !draft) return null;

  const phase = localView ?? snapshot.phase;
  const canExit = snapshot.presentation === "modal" && !snapshot.transaction;
  const rolledBack = snapshot.transaction?.status === "rolled_back";
  const failureText = failure
    ? rolledBack
      ? "Мастер заметил изменение конфигурации извне и остановил настройку до небезопасного применения."
      : failure.code === "RUNTIME_UNAVAILABLE"
      ? "Защищённая служба отсутствует или повреждена. До восстановления мастер ничего не применит."
      : failure.code === "ROLLBACK_FAILED"
        ? "Служба временно не завершила откат. Сохранённый снимок цел — повторите операцию через несколько секунд."
      : failure.code === "STALE_PLAN" || failure.code === "STALE_REVISION"
        ? "Состояние изменилось в другом месте. Мастер перечитает актуальный снимок перед повтором."
        : "Операция остановлена безопасно. Технические детали сохранены только в локальном журнале."
    : "";
  const advisoryFailure = failure?.code === "ROLLBACK_FAILED";

  const renderBody = () => {
    if (phase === "welcome") {
      return (
        <div className="grid gap-3 sm:grid-cols-3">
          <StatusRow label="Локальные данные" ok detail="Draft, checkpoints и результаты остаются на этом компьютере." />
          <StatusRow label="Без изменений заранее" ok detail="До экрана Review ни одна системная настройка не меняется." />
          <StatusRow label="Без повторного UAC" ok detail="Привилегированные действия выполняет уже установленная служба." />
        </div>
      );
    }

    if (phase === "readiness") {
      const rows = readiness
        ? [
            ["Защищённая служба", readiness.service, "Аутентифицированный runtime отвечает"],
            ["DPI capability", readiness.dpi, "Legacy доступен через Program Files"],
            ["Hosts capability", readiness.hosts, "Системный hosts управляется службой"],
            ["Telegram runtime", readiness.telegram, "Локальный proxy binary прошёл manifest"],
            ["Защищённые ресурсы", readiness.protectedResources, "Конфиги читаются из immutable layout"],
            ["AppData", readiness.appData, "Checkpoint можно записать атомарно"],
            ["Порт Telegram", readiness.proxyPort, "Порт свободен или уже принадлежит Obsession"],
            ["Незавершённая операция", !readiness.pendingRecovery, "Нет journal recovery, блокирующего новый apply"],
          ] as const
        : [];
      return (
        <div className="grid gap-2 sm:grid-cols-2">
          {rows.map(([label, ok, detail]) => (
            <StatusRow key={label} label={label} ok={ok} detail={detail} />
          ))}
        </div>
      );
    }

    if (phase === "goals") {
      return (
        <div className="grid gap-3 sm:grid-cols-3">
          <GoalCard id="dpi" title="Discord / DPI" description="Legacy — стабильный вариант; Discord станет первой категорией." checked={draft.goals.dpi} onChange={toggleGoal} />
          <GoalCard id="ai" title="Доступ к ИИ" description="Применить service-owned hosts с атомарным возвратом." checked={draft.goals.ai} onChange={toggleGoal} />
          <GoalCard id="telegram" title="Telegram" description="Запустить локальный MTProto-прокси и проверить endpoint." checked={draft.goals.telegram} onChange={toggleGoal} />
        </div>
      );
    }

    if (phase === "recommendation") {
      return (
        <div className="space-y-3">
          {draft.goals.dpi && (
            <fieldset className="grid gap-3 sm:grid-cols-2">
              <legend className="mb-2 text-xs font-medium uppercase tracking-[0.16em] text-ink-muted">DPI-движок</legend>
              {(["legacy", "zapret2"] as const).map((engine) => (
                <label key={engine} className={`flex cursor-pointer gap-3 rounded-xl border p-4 ${draft.dpiEngine === engine ? "border-accent/55 bg-accent/10" : "border-white/[0.08] bg-base-800/45"}`}>
                  <input type="radio" name="dpi-engine" checked={draft.dpiEngine === engine} onChange={() => setDraft({ ...draft, dpiEngine: engine })} />
                  <span><strong className="block text-sm text-ink">{engine === "legacy" ? "Legacy — рекомендовано" : "Zapret2 — Advanced / Beta"}</strong><small className="mt-1 block text-xs leading-5 text-ink-soft">{engine === "legacy" ? "Предсказуемый основной движок и Reliability строго в observe_only." : "Для ручного эксперимента; adaptive-поиск автоматически не включается."}</small></span>
                </label>
              ))}
            </fieldset>
          )}
          <StatusRow label="Существующая конфигурация сохранена" ok detail="При повторном запуске мастер меняет только явно подтверждённые пункты." />
        </div>
      );
    }

    if (phase === "review") {
      return (
        <ol className="space-y-2">
          {snapshot.plan?.actions.map((action, index) => (
            <li key={action.kind} className="rounded-xl border border-white/[0.08] bg-base-800/45 p-3.5">
              <div className="flex gap-3"><span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-accent/15 text-xs font-semibold text-accent-cyan">{index + 1}</span><span><strong className="text-sm text-ink">{action.title}</strong><small className="mt-1 block text-xs leading-5 text-ink-soft">{action.detail}</small></span></div>
              <div className="mt-2 grid gap-1 pl-10 text-[11px] text-ink-muted sm:grid-cols-2"><span>Проверка: {action.verification}</span><span>Rollback: {action.rollback}</span></div>
            </li>
          ))}
        </ol>
      );
    }

    if (phase === "applying" || phase === "rolling_back" || phase === "verifying") {
      return (
        <div className="flex min-h-48 flex-col items-center justify-center gap-4 text-center" aria-live="polite">
          <motion.span className="h-14 w-14 rounded-full border border-accent/30 border-t-accent" animate={{ rotate: 360 }} transition={{ duration: 1.4, repeat: Infinity, ease: "linear" }} aria-hidden="true" />
          <div><strong className="text-base text-ink">{phase === "verifying" ? "Проверяем только выбранные цели" : phase === "rolling_back" ? "Возвращаем точный снимок" : "Применяем подтверждённый план"}</strong><p className="mt-1 text-xs text-ink-soft">Snapshot остаётся источником истины; закрывать приложение сейчас не нужно.</p></div>
        </div>
      );
    }

    if (phase === "recovery_required") {
      const hasJournal = Boolean(snapshot.transaction);
      return (
        <div className="space-y-3">
          <StatusRow
            label={hasJournal ? "Настройка приостановлена" : "Служба недоступна"}
            ok={false}
            detail={hasJournal
              ? "Мастер остановил дальнейшие действия. Journal помнит выполненные шаги и продолжит обратный откат с последнего checkpoint."
              : "Protected runtime отсутствует или повреждён. Новые изменения не выполнялись."}
          />
          <p className="rounded-xl border border-white/[0.07] bg-base-800/35 p-4 text-xs leading-5 text-ink-soft">
            {hasJournal
              ? "Это не означает, что приложение или runtime сломаны. Сохранённый снимок остаётся целым; безопасный откат можно повторить."
              : <>Repair запускается только по фиксированному пути <code className="font-mono">C:\Program Files\Obsession\uninstall.exe</code> без uninstall-switch. Путь из frontend не принимается.</>}
          </p>
        </div>
      );
    }

    const [title, description] = outcomeCopy(snapshot.verification?.outcome, rolledBack);
    return (
      <div className="space-y-3">
        <div className="rounded-xl border border-white/[0.08] bg-base-800/45 p-4"><strong className="text-base text-ink">{title}</strong><p className="mt-1 text-xs leading-5 text-ink-soft">{description}</p></div>
        <div className="grid gap-2 sm:grid-cols-3">
          {snapshot.verification?.targets.filter((target) => target.status !== "not_selected").map((target) => <StatusRow key={target.id} label={target.label} ok={target.status === "passed"} detail={target.message} />)}
        </div>
      </div>
    );
  };

  const heading = phase === "welcome" ? "Настроим выбранные функции" : phase === "readiness" ? "Проверка готовности" : phase === "goals" ? "Что должно работать сразу?" : phase === "recommendation" ? "Рекомендуем безопасный профиль" : phase === "review" ? "Точный план перед применением" : phase === "recovery_required" ? (snapshot.transaction ? "Нужно завершить безопасный откат" : "Служба требует восстановления") : phase === "result" ? "Результат проверки" : "Obsession настраивает систему";
  const description = phase === "welcome" ? "Мастер работает через уже установленную службу и не запрашивает UAC повторно." : phase === "review" ? "После подтверждения план становится immutable, а перед первой mutation fingerprints проверяются ещё раз." : phase === "recovery_required" && snapshot.transaction ? "Снимок настроек сохранён. Приложение продолжает работать в обычном режиме." : "Состояние и checkpoints сохраняются локально и атомарно.";

  const footer = (() => {
    if (phase === "welcome") return <Button disabled={busy} onClick={() => void checkReadiness().then((ok) => ok && setLocalView("readiness"))} className={primaryButtonClass}>Проверить готовность</Button>;
    if (phase === "readiness") {
      const ready = readiness && readiness.service && readiness.protectedResources && !readiness.pendingRecovery;
      return <Button disabled={busy || !ready} onClick={() => setLocalView("goals")} className={primaryButtonClass}>Выбрать цели</Button>;
    }
    if (phase === "goals") return <Button disabled={busy || selectedCount === 0} onClick={() => void saveDraft(draft).then((ok) => ok && setLocalView(null))} className={primaryButtonClass}>Получить рекомендацию</Button>;
    if (phase === "recommendation") return <Button disabled={busy} onClick={() => void buildPlan(draft)} className={primaryButtonClass}>Собрать точный план</Button>;
    if (phase === "review") return <Button disabled={busy} onClick={() => void apply().then((ok) => { if (ok) void verify(); })} className={primaryButtonClass}>Применить и проверить</Button>;
    if (phase === "recovery_required") {
      if (snapshot.transaction && readiness?.service !== false) {
        return <Button disabled={busy} onClick={() => void rollback()} className={primaryButtonClass}>Повторить безопасный откат</Button>;
      }
      return <Button disabled={busy || readiness?.repairAvailable === false} onClick={() => void launchRepair()} className={primaryButtonClass}>Восстановить службу</Button>;
    }
    if (phase !== "result") return null;
    const outcome = snapshot.verification?.outcome;
    const finish = async () => {
      const destination = destinationFor(draft);
      if (await complete(destination)) onDestination?.(destination);
    };
    if (outcome === "success" || snapshot.verification?.accepted || rolledBack) return <Button disabled={busy} onClick={() => void finish()} className={primaryButtonClass}>{rolledBack ? "Завершить без изменений" : "Открыть приложение"}</Button>;
    return <div className="flex flex-wrap justify-end gap-2"><Button variant="ghost" disabled={busy} onClick={() => void verify()}>Повторить проверку</Button><Button variant="ghost" disabled={busy} onClick={() => void rollback()}>Откатить</Button><Button disabled={busy} onClick={() => void acceptVerification().then((ok) => { if (ok) void finish(); })} className={primaryButtonClass}>Оставить конфигурацию</Button></div>;
  })();

  return (
    <div className="no-drag fixed inset-0 z-[100] flex items-center justify-center bg-black/75 px-4 py-6 backdrop-blur-xl">
      <motion.div ref={dialogRef} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descriptionId} initial={{ opacity: 0, y: 14, scale: 0.985 }} animate={{ opacity: 1, y: 0, scale: 1 }} transition={spring.soft} style={{ width: PANEL_WIDTH }} className="relative flex max-h-[calc(100vh-3rem)] flex-col overflow-hidden rounded-2xl border border-white/[0.1] bg-base-900/95 shadow-2xl">
        <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-accent-cyan/45 to-transparent" />
        <header className="flex shrink-0 items-start justify-between gap-4 border-b border-white/[0.07] px-5 py-4 sm:px-6">
          <div><p className="mb-1.5 font-mono text-[10px] uppercase tracking-[0.18em] text-accent-cyan">Onboarding V2 · {phase}</p><h1 ref={headingRef} id={titleId} tabIndex={-1} className="text-xl font-semibold tracking-tight text-ink outline-none">{heading}</h1><p id={descriptionId} className="mt-1.5 max-w-2xl text-xs leading-5 text-ink-soft">{description}</p></div>
          {canExit && <button type="button" onClick={() => void cancel()} disabled={busy} aria-label="Закрыть мастер" className="rounded-lg p-2 text-ink-muted hover:bg-white/10 hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent">×</button>}
        </header>
        <main className="min-h-0 overflow-y-auto px-5 py-4 sm:px-6" style={{ maxHeight: BODY_MAX_HEIGHT }}>{renderBody()}{failureText && <div role={rolledBack ? "status" : "alert"} className={`mt-3 rounded-xl border px-4 py-3 text-xs leading-5 ${rolledBack ? "border-white/[0.09] bg-white/[0.035] text-ink-soft" : advisoryFailure ? "border-warn/25 bg-warn/10 text-warn" : "border-danger/25 bg-danger/10 text-danger"}`}>{failureText}{failure?.logPath && <span className="mt-1 block break-all font-mono text-[10px] text-ink-muted">{failure.logPath}</span>}</div>}</main>
        <footer className="flex min-h-[68px] shrink-0 items-center justify-between gap-3 border-t border-white/[0.07] px-5 py-3 sm:px-6"><div>{!snapshot.transaction && snapshot.presentation === "required" && <Button variant="ghost" disabled={busy} onClick={() => void skip()}>Пропустить</Button>}</div><div className="ml-auto">{footer}</div></footer>
      </motion.div>
    </div>
  );
}
