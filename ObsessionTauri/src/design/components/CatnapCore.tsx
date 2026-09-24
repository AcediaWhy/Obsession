import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { useRenderActive } from "../render";
import { CoreShell } from "./CoreShell";
import { CatnapSpriteLab } from "./CatnapSpriteLab";

type Props = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  interactive?: boolean;
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

// Пиксельное превью темы Catnap.
export function CatnapCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  onClick,
  size = 240,
  paused = false,
  interactive = true,
}: Props) {
  const motionOn = useRenderActive() && !paused;
  const phase = phaseForSignals({ active, busy, scanning, alarm });

  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <div
        aria-hidden="true"
        className="catnap-core"
        data-catnap-core
        style={{ width: size, height: size }}
      >
        <CatnapSpriteLab phase={phase} paused={!motionOn} size={size} />
      </div>
    </CoreShell>
  );
}
