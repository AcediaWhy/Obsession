import { BlackPondCatSprite } from "./axolotl/BlackPondCatSprite";
import { SunkenStarFieldLab } from "./SunkenStarFieldLab";
import { useMotionOff, useRenderHidden } from "../render";
import { useObsessionVisualPhase } from "../useObsessionVisualPhase";
import "../../styles/quietPond.css";

export function QuietPondField({ paused = false }: { paused?: boolean }) {
  const motionOff = useMotionOff();
  return (
    <>
      <SunkenStarFieldLab paused={paused || motionOff} />
      <div className="quietpond-veil absolute inset-0" />
    </>
  );
}

export function QuietPondCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  paused = false,
  size = 240,
  onClick,
  interactive = true,
}: {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  paused?: boolean;
  size?: number;
  onClick: () => void;
  interactive?: boolean;
}) {
  const livePhase = useObsessionVisualPhase();
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  const phase = interactive
    ? alarm ? "fault" : busy ? "engaging" : scanning ? "scanning" : livePhase === "idle" && active ? "focused" : livePhase
    : active ? "focused" : "idle";
  const sprite = (
    <BlackPondCatSprite
      size={size}
      phase={phase}
      paused={paused || motionOff || hidden}
    />
  );

  if (!interactive) {
    return <div className="quietpond-core" style={{ width: size, height: size }}>{sprite}</div>;
  }

  return (
    <button
      type="button"
      className="quietpond-core"
      aria-label={active ? "Отключить защиту" : "Активировать защиту"}
      disabled={busy}
      onClick={onClick}
      style={{ width: size, height: size }}
    >
      {sprite}
    </button>
  );
}
