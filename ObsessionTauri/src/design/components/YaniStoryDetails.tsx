import type { EarMood } from "./yaniEar/types";
import "./YaniStoryDetails.css";

export type YaniStoryDensity = "off" | "quiet" | "story" | "chaotic";
export type YaniStoryDepth = "flat" | "layered" | "deep";

type Props = {
  density: Exclude<YaniStoryDensity, "off">;
  depth: YaniStoryDepth;
  mood: EarMood;
};

export function YaniStoryDetails({ density, depth, mood }: Props) {
  return (
    <div
      aria-hidden="true"
      className="yani-story-details"
      data-density={density}
      data-depth={depth}
      data-mood={mood}
    >
      {density === "chaotic" && (
        <>
          <span className="yani-story-details__world-status">WORLD KNOWS: TRUE</span>
          <span className="yani-story-details__pancake">
            <i /><i /><i />
          </span>
          <span className="yani-story-details__glitch yani-story-details__glitch--a" />
          <span className="yani-story-details__glitch yani-story-details__glitch--b" />
          <span className="yani-story-details__glitch yani-story-details__glitch--c" />
        </>
      )}
    </div>
  );
}
