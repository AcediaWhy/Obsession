import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import { BlackCatIdle } from "./BlackCatIdle";

type BlackPondCatSpriteProps = {
  paused?: boolean;
  phase: ObsessionVisualPhase;
  size: number;
};

export function BlackPondCatSprite({ paused = false, phase, size }: BlackPondCatSpriteProps) {
  if (phase === "idle" && !paused) return <BlackCatIdle size={size} />;

  // Only these two source resolutions are needed by the shipped UI. Larger lab
  // previews scale the 256 px artwork without adding more release assets.
  const assetSize = size <= 128 ? 128 : 256;
  const fileName = `${phase}-${assetSize}${paused ? "-still" : ""}.webp`;

  return (
    <img
      alt=""
      aria-hidden="true"
      className="black-pond-cat-sprite"
      data-black-pond-cat-sprite="true"
      data-motion={paused ? "paused" : "running"}
      data-phase={phase}
      draggable={false}
      src={`${import.meta.env.BASE_URL}lab-assets/black-cat-states/${fileName}`}
      style={{ height: size, width: size }}
    />
  );
}
