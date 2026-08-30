import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { useRenderActive } from "../render";
import { CoreShell } from "./CoreShell";
import { OphanimCatSpriteLab } from "./OphanimCatSpriteLab";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  interactive?: boolean;
  // Реактивная телеметрия щита (передаёт hero на экране DPI; превью — нет).
  scanning?: boolean;
  alarm?: boolean;
};

function phaseForSignals({
  active,
  busy,
  scanning,
  alarm,
}: Pick<Props, "active" | "busy" | "scanning" | "alarm">): ObsessionVisualPhase {
  if (alarm) return "fault";
  if (scanning) return "scanning";
  if (busy) return "engaging";
  if (active) return "focused";
  return "idle";
}

// Пиксельный Офаним: кот-ступица, хвост-кольцо и Великое Око.
// Сигнатура совпадает с остальными ядрами, поэтому один renderer работает
// и как интерактивный hero, и как декоративное превью темы.
export function OphanimCore({
  active,
  busy = false,
  onClick,
  size = 240,
  paused = false,
  interactive = true,
  scanning = false,
  alarm = false,
}: Props) {
  const motionOn = useRenderActive() && !paused;
  const phase = phaseForSignals({ active, busy, scanning, alarm });

  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <div
        aria-hidden="true"
        className="ophanim-cat-core"
        data-ophanim-cat-core
        style={{ width: size, height: size }}
      >
        <OphanimCatSpriteLab phase={phase} paused={!motionOn} size={size} />
      </div>
    </CoreShell>
  );
}
