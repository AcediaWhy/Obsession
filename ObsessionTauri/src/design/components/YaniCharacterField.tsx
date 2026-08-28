import type { ObsessionVisualPhase } from "../obsessionVisualState";
import type { QualityTier } from "../render";
import { YaniCharacterScene } from "./YaniCharacterScene";
import {
  YaniStoryDetails,
  type YaniStoryDensity,
  type YaniStoryDepth,
} from "./YaniStoryDetails";
import type { EarMood, YaniArtPass } from "./yaniEar/types";
import "./YaniCharacterField.css";

type Props = {
  phase?: ObsessionVisualPhase;
  screen?: string;
  paused?: boolean;
  qualityTier?: QualityTier;
  forceFallback?: boolean;
  storyDensity?: YaniStoryDensity;
  storyDepth?: YaniStoryDepth;
  /** Художественный проход. Приложение проп не передаёт — только лаба. */
  art?: YaniArtPass;
};

export function yaniCharacterMood(phase: ObsessionVisualPhase): EarMood {
  if (phase === "engaging") return "busy";
  if (phase === "scanning") return "scanning";
  if (phase === "focused") return "active";
  if (phase === "fault") return "alarm";
  return "idle";
}

export function YaniCharacterField({
  phase = "idle",
  screen = "overview",
  paused = false,
  qualityTier = "balanced",
  forceFallback = false,
  storyDensity = "off",
  storyDepth = "layered",
  art = "current",
}: Props) {
  const mood = yaniCharacterMood(phase);

  return (
    <div
      aria-hidden="true"
      className="yani-character-field"
      data-mood={mood}
      data-screen={screen}
      data-model={forceFallback ? "fallback" : "three"}
      data-motion={paused ? "still" : "running"}
      data-story={storyDensity}
      data-depth={storyDepth}
      data-art={art}
    >
      <div className="yani-character-field__ambient" />
      <div className="yani-character-field__sweep" />
      <div className="yani-character-field__room-depth">
        <span className="yani-character-field__tail-echo" />
        <span className="yani-character-field__purr-waves"><i /><i /><i /></span>
        <span className="yani-character-field__whisker-field"><i /><i /><i /><i /></span>
        <span className="yani-character-field__breath-glow" />
        <span className="yani-character-field__cat-signals">
          <i>EAR L / LISTENING</i>
          <i>TAIL LOOP / 09</i>
          <i>PURR / 07Hz</i>
        </span>
        <svg className="yani-character-field__cat-scribble" viewBox="0 0 60 58">
          <path d="M8 21 7 5l13 8c7-3 14-3 21 0l13-8-2 16c5 5 7 12 4 19-4 10-14 15-26 15S8 50 4 40c-3-7-1-14 4-19Z" />
          <path d="M17 31h1m23 0h1M26 39c2 3 6 3 9 0M2 33l13 3M1 40l14 1m43-8-13 3m14 4-14 1" />
        </svg>
        <span className="yani-character-field__room-mark">OBS // YANI · 03:17</span>
        <span className="yani-character-field__paw-trail"><i /><i /><i /></span>
      </div>
      <div className="yani-character-field__details">
        <span className="yani-character-field__panel yani-character-field__panel--a" />
        <span className="yani-character-field__panel yani-character-field__panel--b" />
        <span className="yani-character-field__panel yani-character-field__panel--c" />
        <span className="yani-character-field__orbit" />
        <span className="yani-character-field__particles" />
      </div>
      <div className="yani-character-field__poster" />
      {storyDensity !== "off" && (
        <YaniStoryDetails density={storyDensity} depth={storyDepth} mood={mood} />
      )}
      {!forceFallback && (
        <YaniCharacterScene
          mood={mood}
          quality={qualityTier}
          paused={paused}
          screen={screen}
          art={art}
          className="yani-character-field__model"
        />
      )}
      <div className="yani-character-field__foreground-depth">
        <span className="yani-character-field__tail-wisp" />
      </div>
      <div className="yani-character-field__grain" />
    </div>
  );
}
