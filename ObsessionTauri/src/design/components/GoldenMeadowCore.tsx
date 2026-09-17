import { BlackPondCatSprite } from "./axolotl/BlackPondCatSprite";
import { useMotionOff, useRenderHidden } from "../render";
import { useObsessionVisualPhase } from "../useObsessionVisualPhase";

type CoreProps = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  onClick?: () => void;
  size?: number;
  interactive?: boolean;
  paused?: boolean;
};

// Use the original animated black cat in every placement of this theme.
export function GoldenMeadowCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  onClick,
  size = 240,
  interactive = true,
  paused = false,
}: CoreProps) {
  const livePhase = useObsessionVisualPhase();
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  const phase = interactive
    ? alarm ? "fault" : busy ? "engaging" : scanning ? "scanning" : livePhase === "idle" && active ? "focused" : livePhase
    : active ? "focused" : "idle";
  const sprite = <BlackPondCatSprite size={size} phase={phase} paused={paused || motionOff || hidden} />;

  if (!interactive) {
    return <div className="golden-meadow-core" style={{ width: size, height: size }}>{sprite}</div>;
  }

  return (
    <button
      type="button"
      aria-label={active ? "Отключить защиту" : "Активировать защиту"}
      disabled={busy}
      onClick={onClick}
      className="golden-meadow-core"
      style={{ width: size, height: size }}
    >
      {sprite}
    </button>
  );
}
