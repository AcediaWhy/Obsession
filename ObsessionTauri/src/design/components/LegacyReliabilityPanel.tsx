import type { LegacyReliabilityPhase } from "../../lib/tauri";
import { useLegacyReliabilityStore } from "../../store/legacyReliabilityStore";
import { SectionLabel } from "./atoms";

export type LegacyReliabilityTone = "muted" | "ok" | "warn" | "danger";

export interface LegacyReliabilityDisplayModel {
  label: string;
  tone: LegacyReliabilityTone;
}

const PHASE_DISPLAY = {
  inactive: { label: "Ожидание запуска", tone: "muted" },
  starting: { label: "Запуск наблюдения", tone: "muted" },
  observing: { label: "Наблюдение", tone: "ok" },
  degraded: { label: "Наблюдение ограничено", tone: "warn" },
  blind: { label: "Наблюдение недоступно", tone: "danger" },
} as const satisfies Record<
  LegacyReliabilityPhase,
  LegacyReliabilityDisplayModel
>;

const TONE_CLASS: Record<LegacyReliabilityTone, string> = {
  muted: "text-ink-muted",
  ok: "text-ok",
  warn: "text-warn",
  danger: "text-danger",
};

export function getLegacyReliabilityDisplayModel(
  phase: LegacyReliabilityPhase,
): LegacyReliabilityDisplayModel {
  return PHASE_DISPLAY[phase];
}

export function LegacyReliabilityPanelView({
  phase,
}: {
  phase: LegacyReliabilityPhase;
}) {
  const display = getLegacyReliabilityDisplayModel(phase);

  return (
    <div className="w-full">
      <SectionLabel>Контроль надёжности</SectionLabel>
      <div className="flex flex-col gap-1 rounded-lg bg-white/5 px-3 py-2 text-sm">
        <div className="flex items-center justify-between gap-3">
          <span className="text-ink-muted">Режим</span>
          <span className="font-semibold text-ink-soft">
            Только наблюдение
          </span>
        </div>
        <div className="flex items-center justify-between gap-3">
          <span className="text-ink-muted">Состояние</span>
          <span className={`text-right font-semibold ${TONE_CLASS[display.tone]}`}>
            {display.label}
          </span>
        </div>
      </div>
      <p className="mt-2 px-1 text-2xs leading-relaxed text-ink-muted">
        Сбои фиксируются, конфигурации не меняются.
      </p>
    </div>
  );
}

export function LegacyReliabilityPanel() {
  const phase = useLegacyReliabilityStore((state) => state.status.phase);
  return <LegacyReliabilityPanelView phase={phase} />;
}
