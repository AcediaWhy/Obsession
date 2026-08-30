import type { CSSProperties } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./AuroraCatSpriteLab.css";

export type AuroraCatMood = "idle" | "busy" | "scanning" | "active" | "alarm";
export type AuroraCatDetail = "auto" | "base" | "hero";

export function auroraCatMoodForPhase(phase: ObsessionVisualPhase): AuroraCatMood {
  if (phase === "fault") return "alarm";
  if (phase === "focused") return "active";
  return phase === "engaging" ? "busy" : phase;
}

type AuroraCatSpriteLabProps = {
  phase: ObsessionVisualPhase;
  detail?: AuroraCatDetail;
  paused?: boolean;
  size?: number;
};

export function AuroraCatSpriteLab({
  phase,
  detail = "auto",
  paused = false,
  size = 240,
}: AuroraCatSpriteLabProps) {
  const resolvedDetail = detail === "auto" ? (size >= 160 ? "hero" : "base") : detail;
  const mood = auroraCatMoodForPhase(phase);
  const viewBox = "0 0 52 52";

  return (
    <svg
      aria-hidden="true"
      className="aurora-cat-sprite-lab"
      data-detail={resolvedDetail}
      data-motion={paused ? "paused" : "running"}
      data-mood={mood}
      focusable="false"
      shapeRendering="crispEdges"
      style={{ "--aurora-cat-size": `${size}px` } as CSSProperties}
      viewBox={viewBox}
    >
      <g className={`aurora-cat-sprite-lab__scene aurora-cat-sprite-lab__scene--${resolvedDetail}`}>
        {/* Ambient Sky Sparkles & Micro Stars */}
        <g className="aurora-cat-sprite-lab__stars">
          <path className="aurora-cat-sprite-lab__star aurora-cat-sprite-lab__star--1" d="M4 18h1v1H4ZM48 12h1v1h-1ZM21 2h1v1h-1ZM35 2h1v1h-1Z" />
          <path className="aurora-cat-sprite-lab__star aurora-cat-sprite-lab__star--2" d="M7 23h1v1H7ZM47 25h1v1h-1ZM29 1h1v1h-1ZM13 4h1v1h-1ZM43 28h1v1h-1Z" />
        </g>

        {/* Left Golden Polar Star */}
        <g className="aurora-cat-sprite-lab__guiding-star">
          <path
            className="aurora-cat-sprite-lab__star-outline"
            d="M5 6h3v1h1v3H8v1H5v-1H4V7h1Z"
          />
          <path
            className="aurora-cat-sprite-lab__star-core"
            d="M6 7h1v3H6ZM5 8h3v1H5Z"
          />
          <path
            className="aurora-cat-sprite-lab__star-spark"
            d="M6 8h1v1H6Z"
          />
        </g>

        {/* Right Star Cluster (Balancing the sky where the droop was) */}
        <g className="aurora-cat-sprite-lab__right-stars">
          {/* Main 4-point Golden Star */}
          <path
            className="aurora-cat-sprite-lab__star-outline"
            d="M44 14h3v1h1v3h-1v1h-3v-1h-1v-3h1Z"
          />
          <path
            className="aurora-cat-sprite-lab__star-core"
            d="M45 15h1v3h-1ZM44 16h3v1h-3Z"
          />
          <path
            className="aurora-cat-sprite-lab__star-spark"
            d="M45 16h1v1h-1Z"
          />
          {/* Accent diamond sparkles */}
          <path
            className="aurora-cat-sprite-lab__star-spark"
            d="M47 20h2v1h-2ZM42 22h1v1h-1Z"
          />
        </g>

        {/* Broad Floating Celestial Aurora Canopy (Clean horizontal arch) */}
        <g className="aurora-cat-sprite-lab__tail-ribbon">
          {/* Layer 1: Cosmic Violet Upper Canopy */}
          <path
            className="aurora-cat-sprite-lab__ribbon aurora-cat-sprite-lab__ribbon--violet"
            d="M7 8h4V5h6V3h16v2h7v2h5v3h-4v-1h-5V7h-7V5H19v2h-6v2H7Z"
          />

          {/* Layer 2: Electric Cyan Radiant Flow Body */}
          <path
            className="aurora-cat-sprite-lab__ribbon aurora-cat-sprite-lab__ribbon--cyan"
            d="M8 9h4V6h6V4h16v2h7v2h5v3h-4v-1h-5V8h-7V6H20v2h-6v2H8Z"
          />

          {/* Layer 3: Emerald / Mint Luminous Under-Glow */}
          <path
            className="aurora-cat-sprite-lab__ribbon aurora-cat-sprite-lab__ribbon--teal"
            d="M10 10h4V8h6V6h14v2h7v2h4v3h-4v-1h-5V9h-6V8H21v2h-6v2h-5Z"
          />

          {/* Stardust Sparkles on the Aurora Canopy */}
          <path
            className="aurora-cat-sprite-lab__ribbon-glint aurora-cat-sprite-lab__ribbon-glint--a"
            d="M18 3h2v1h-2ZM32 3h2v1h-2ZM42 5h2v1h-2Z"
          />
          <path
            className="aurora-cat-sprite-lab__ribbon-glint aurora-cat-sprite-lab__ribbon-glint--b"
            d="M25 4h2v1h-2ZM37 4h2v1h-2Z"
          />
        </g>

        {/* Soft Organic Ground & Paw Contact Shadows */}
        <g className="aurora-cat-sprite-lab__snow">
          <path
            className="aurora-cat-sprite-lab__snow-shadow"
            d="M15 44h8v1h-8ZM29 44h8v1h-8ZM36 44h6v1h-6ZM16 45h5v1h-5ZM30 45h5v1h-5ZM37 45h4v1h-4Z"
          />
          <path
            className="aurora-cat-sprite-lab__snow-ambient"
            d="M13 45h26v1H13ZM18 46h16v1H18Z"
          />
        </g>

        {/* Detailed Kawaii White Polar Cat */}
        <g className="aurora-cat-sprite-lab__cat">
          {/* Fluffy Curled Tail (on right) */}
          <g className="aurora-cat-sprite-lab__tail-group">
            <path
              className="aurora-cat-sprite-lab__tail-outline"
              d="M37 32h4v2h2v2h1v5h-1v2h-2v2h-3v1h-3v-2h3v-1h2v-2h1v-4h-1v-2h-2v-1h-4v-2Z"
            />
            <path
              className="aurora-cat-sprite-lab__tail-fur"
              d="M38 34h2v2h1v5h-1v2h-2v1h-1v-1h1v-2h1v-4h-1v-2h-2v-1h1Z"
            />
            <path
              className="aurora-cat-sprite-lab__tail-light"
              d="M39 34h1v4h-1Z"
            />
          </g>

          {/* Body & Paws */}
          <g className="aurora-cat-sprite-lab__body">
            {/* Outline */}
            <path
              className="aurora-cat-sprite-lab__cat-outline"
              d="M16 33h20v2h2v10h-2v1H16v-1h-2V35h2Z"
            />
            {/* White Fur Base */}
            <path
              className="aurora-cat-sprite-lab__cat-fur"
              d="M16 35h20v9H16Z"
            />
            {/* Lavender Body & Leg Shadows */}
            <path
              className="aurora-cat-sprite-lab__cat-shadow"
              d="M16 35h2v9h-2ZM34 35h2v9h-2ZM25 37h2v7h-2ZM21 43h3v1h-3ZM28 43h3v1h-3Z"
            />
            {/* White Chest Light & Bib */}
            <path
              className="aurora-cat-sprite-lab__cat-light"
              d="M18 35h7v8h-7ZM27 35h7v8h-7ZM24 35h4v3h-4Z"
            />
            {/* Paw Pads (Soft Pink) */}
            <path
              className="aurora-cat-sprite-lab__paw"
              d="M17 43h3v1h-3ZM32 43h3v1h-3Z"
            />
          </g>

          {/* Head */}
          <g className="aurora-cat-sprite-lab__head">
            {/* Head & Ear Outlines */}
            <path
              className="aurora-cat-sprite-lab__cat-outline"
              d="M16 15h5v2h1v2h8v-2h1v-2h5v6h1v2h2v3h-1v2h1v3h-2v2h-3v1H17v-1h-3v-2h-2v-3h1v-2h-1v-3h2v-2h1Z"
            />
            {/* White Head Fur */}
            <path
              className="aurora-cat-sprite-lab__cat-fur"
              d="M17 17h3v2h2v2h8v-2h2v-2h3v5h2v3h1v3h-1v2h-1v2h-2v1H18v-1h-2v-2h-1v-2h-1v-3h1v-3h2Z"
            />
            {/* Lavender Forehead Tufts & Shadows */}
            <path
              className="aurora-cat-sprite-lab__cat-shadow"
              d="M23 19h2v4h-2ZM27 19h2v4h-2ZM25 18h2v2h-2ZM15 28h1v3h-1ZM36 28h1v3h-1ZM18 32h16v1H18Z"
            />
            {/* Inner Ears (Pink) with white fur fluff */}
            <path
              className="aurora-cat-sprite-lab__inner-ear"
              d="M18 17h2v3h-2ZM32 17h2v3h-2Z"
            />
            <path
              className="aurora-cat-sprite-lab__cat-light"
              d="M19 19h1v1h-1ZM32 19h1v1h-1Z"
            />

            {/* Cheeks (Blush) */}
            <path
              className="aurora-cat-sprite-lab__blush"
              d="M15 28h3v2h-3ZM34 28h3v2h-3ZM16 30h2v1h-2ZM34 30h2v1h-2Z"
            />

            {/* Eyes */}
            {/* Sleep Mode: Happy curved smiling eyes */}
            <path
              className="aurora-cat-sprite-lab__eye aurora-cat-sprite-lab__eye--sleep"
              d="M18 26h4v1h-4ZM19 25h2v1h-2ZM17 26h1v1h-1ZM30 26h4v1h-4ZM31 25h2v1h-2ZM34 26h1v1h-1Z"
            />
            {/* Awake Mode: Sparkling Kawaii Eyes */}
            <g className="aurora-cat-sprite-lab__eye-awake-group">
              <path
                className="aurora-cat-sprite-lab__eye aurora-cat-sprite-lab__eye--awake"
                d="M18 24h4v4h-4ZM30 24h4v4h-4Z"
              />
              <path
                className="aurora-cat-sprite-lab__eye-pupil"
                d="M19 25h3v3h-3ZM31 25h3v3h-3Z"
              />
              <path
                className="aurora-cat-sprite-lab__eye-glint"
                d="M18 24h1v1h-1ZM30 24h1v1h-1ZM20 27h1v1h-1ZM32 27h1v1h-1Z"
              />
            </g>

            {/* Cute Muzzle (:3) and Pink Nose */}
            <path className="aurora-cat-sprite-lab__nose" d="M25 27h2v1h-2Z" />
            <path
              className="aurora-cat-sprite-lab__mouth"
              d="M23 29h2v1h-2ZM27 29h2v1h-2ZM25 30h2v1h-2Z"
            />
          </g>

          {/* Polar Crystal Collar */}
          <path className="aurora-cat-sprite-lab__collar" d="M18 34h16v1H18Z" />
          <path className="aurora-cat-sprite-lab__collar-gem" d="M25 35h2v2h-2Z" />
        </g>

        {/* State Indicators */}
        <path className="aurora-cat-sprite-lab__scan-spark" d="M41 16h1v2h2v1h-2v2h-1v-2h-2v-1h2Z" />
        <path className="aurora-cat-sprite-lab__alarm-mark" d="M5 32h2v5H5Zm0 7h2v2H5Z" />
      </g>
    </svg>
  );
}
