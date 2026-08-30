import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./MidnightSpriteLab.css";

export type MidnightDetail = "auto" | "base" | "hero";

export type MidnightSpriteLabProps = {
  phase?: ObsessionVisualPhase;
  detail?: MidnightDetail;
  paused?: boolean;
  size?: number | string;
};

type Mood = "idle" | "busy" | "scanning" | "active" | "alarm";

function moodForPhase(phase: ObsessionVisualPhase): Mood {
  switch (phase) {
    case "idle":
      return "idle";
    case "engaging":
      return "busy";
    case "scanning":
      return "scanning";
    case "focused":
      return "active";
    case "fault":
      return "alarm";
    default:
      return "idle";
  }
}

function resolveDetail(detail: MidnightDetail, size: number | string): "base" | "hero" {
  if (detail === "base" || detail === "hero") return detail;
  const numeric = typeof size === "number" ? size : Number.parseInt(size, 10);
  return Number.isNaN(numeric) || numeric >= 180 ? "hero" : "base";
}

export function MidnightSpriteLab({
  phase = "idle",
  detail = "auto",
  paused = false,
  size = 240,
}: MidnightSpriteLabProps) {
  const mood = moodForPhase(phase);
  const resolvedDetail = resolveDetail(detail, size);
  const motion = paused ? "paused" : "running";
  const sizeValue = typeof size === "number" ? `${size}px` : size;

  return (
    <svg
      aria-label="Midnight ornate vintage streetlamp and perfect cozy cat in night scarf pixel art sprite"
      className="midnight-sprite-lab"
      data-detail={resolvedDetail}
      data-midnight-sprite-lab
      data-mood={mood}
      data-motion={motion}
      data-phase={phase}
      role="img"
      shapeRendering="crispEdges"
      style={{ ["--midnight-core-size" as string]: sizeValue }}
      viewBox="0 0 52 52"
    >
      <g className={`midnight-sprite-lab__scene midnight-sprite-lab__scene--${resolvedDetail}`}>
        {/* Ambient Distant Stars */}
        <g className="midnight-sprite-lab__stars">
          <path className="midnight-sprite-lab__star midnight-sprite-lab__star--1" d="M4 14h1v1H4ZM48 10h1v1h-1ZM3 34h1v1H3ZM48 38h1v1h-1Z" />
          <path className="midnight-sprite-lab__star-cross" d="M46 16h1v3h-1ZM45 17h3v1h-3ZM5 24h1v3H5ZM4 25h3v1H4Z" />
        </g>

        {/* Ambient Streetlamp Light Halo */}
        <g className="midnight-sprite-lab__lamp-halo-group">
          <path
            className="midnight-sprite-lab__lamp-halo"
            d="M20 0h12v2h4v4h3v4h-3v4h-4v2H20v-2h-4v-4h-3V6h3V2h4V0Z"
          />
          <path
            className="midnight-sprite-lab__lamp-mid"
            d="M22 1h8v2h3v3h2v3h-2v3h-3v2h-8v-2h-3v-3h-2V6h2V3h3V1Z"
          />
        </g>

        {/* Soft Volumetric Light Cone illuminating the Cat */}
        <g className="midnight-sprite-lab__light-cone-group">
          <path
            className="midnight-sprite-lab__light-cone"
            d="M22 9h8v3h3v4h3v5h3v5h3v5h3v5h2v6H9v-6h2v-5h3v-5h3v-5h3v-5h3v-4h3v-3Z"
          />
          <path
            className="midnight-sprite-lab__light-core"
            d="M24 9h4v3h2v4h2v5h2v5h2v5h2v5h2v6H17v-6h2v-5h2v-5h2v-5h2v-5h2v-4h2v-3Z"
          />
        </g>

        {/* Detailed Ornate Victorian Streetlamp Architecture (Top Arch) */}
        <g className="midnight-sprite-lab__streetlamp-group">
          {/* 1. Iron Pole on the Left with base and decorative finial collar */}
          <path
            className="midnight-sprite-lab__iron"
            d="M5 40h4v2H5ZM6 38h2v2H6ZM6 18h2v20H6ZM5 16h4v2H5ZM6 6h2v10H6Z"
          />
          {/* 2. Ornate Curving Arch Overhead Bracket & Scrollwork */}
          <path
            className="midnight-sprite-lab__iron"
            d="M7 4h3v2H7ZM10 2h4v2h-4ZM14 1h6v1h-6ZM20 0h7v1h-7ZM11 5h2v2h-2ZM13 7h2v2h-2ZM10 7h1v1h-1ZM12 9h1v1h-1Z"
          />
          {/* 3. Lantern Suspension Ring & Top Gothic Hood */}
          <path
            className="midnight-sprite-lab__iron"
            d="M25 0h2v1h-2ZM23 1h6v1h-6ZM21 2h10v1h-10ZM20 3h12v1h-12ZM19 4h14v1h-14Z"
          />
          {/* 4. Glowing Glass Chamber with 3 Panes & Iron Mullions */}
          <path
            className="midnight-sprite-lab__lamp-glass"
            d="M21 5h10v4h-1v1h-8v-1h-1V5Z"
          />
          <path
            className="midnight-sprite-lab__lamp-filament"
            d="M24 6h4v2h-4Z"
          />
          <path
            className="midnight-sprite-lab__iron"
            d="M21 5h1v4h-1ZM30 5h1v4h-1ZM24 5h1v4h-1ZM27 5h1v4h-1Z"
          />
          {/* 5. Tapered Lower Iron Housing & Decorative Bottom Pendant */}
          <path
            className="midnight-sprite-lab__iron"
            d="M22 9h8v1h-8ZM23 10h6v1h-6ZM25 11h2v2h-2Z"
          />
        </g>

        {/* Soft Ground Shadow beneath the Cat & Pole */}
        <g className="midnight-sprite-lab__ground-shadow-group">
          <path
            className="midnight-sprite-lab__ground-shadow"
            d="M4 41h6v2H4ZM16 41h20v2H16ZM18 43h16v1H18Z"
          />
          <path
            className="midnight-sprite-lab__ground-ambient"
            d="M13 42h26v1H13Z"
          />
        </g>

        {/* Outlined Pixel "Z Z Z" Dreams (in Idle state) */}
        <g className="midnight-sprite-lab__dream-zzz">
          {/* Small Z */}
          <g className="midnight-sprite-lab__zzz-group">
            <path
              className="midnight-sprite-lab__zzz-outline"
              d="M34 16h5v2h-1v1h2v2h-6v-2h2v-1h-2v-2Z"
            />
            <path
              className="midnight-sprite-lab__zzz-core"
              d="M35 17h3v1h-3ZM36 18h2v1h-2ZM35 19h3v1h-3Z"
            />
          </g>
          {/* Mid Z */}
          <g className="midnight-sprite-lab__zzz-group">
            <path
              className="midnight-sprite-lab__zzz-outline"
              d="M38 10h6v2h-2v1h-1v-1h-1v1h3v2h-7v-2h3v-1h-1v-1h-2v-2Z"
            />
            <path
              className="midnight-sprite-lab__zzz-core"
              d="M39 11h4v1h-4ZM41 12h2v1h-2ZM40 13h2v1h-2ZM39 14h4v1h-4Z"
            />
          </g>
        </g>

        {/* Active Mode: Dancing Streetlight Sparkles */}
        <g className="midnight-sprite-lab__active-sparks">
          <path
            className="midnight-sprite-lab__active-spark"
            d="M36 10h1v3h-1ZM35 11h3v1h-3ZM44 6h1v3h-1ZM43 7h3v1h-3ZM42 16h1v2h-1ZM41 17h3v1h-3Z"
          />
        </g>

        {/* The Exact Reference 1 Chubby Fluffy Cat in Midnight Theme */}
        <g className="midnight-sprite-lab__cat">
          {/* 1. Tail Group (Curled on Right) */}
          <g className="midnight-sprite-lab__tail-group">
            <path
              className="midnight-sprite-lab__cat-outline"
              d="M36 32h5v-3h2v-3h4v8h-2v3h-2v2h-7v-2h2v-2h-2Z"
            />
            <path
              className="midnight-sprite-lab__cat-body"
              d="M37 33h4v-3h2v-2h2v6h-2v3h-2v1h-4Z"
            />
            {/* Scarf-Blue Stripe Accent */}
            <path
              className="midnight-sprite-lab__cat-tail-stripe"
              d="M40 30h2v2h-2ZM38 35h3v2h-3Z"
            />
            {/* White Tail Tip */}
            <path
              className="midnight-sprite-lab__cat-white"
              d="M42 27h2v2h-2Z"
            />
          </g>

          {/* 2. Main Body & Face Structure */}
          <g className="midnight-sprite-lab__body">
            {/* 1px Dark Contour Rim behind the fur */}
            <path
              className="midnight-sprite-lab__cat-outline"
              d="M17 9h6v3h6V9h6v7h2v2h2v3h-2v2h3v2h-3v2h2v3h-2v2h-1v2h2v4h-2v3h-2v2h-3v2H19v-2h-3v-2h-2v-3h-2v-4h2v-2h-1v-2h-2v-3h2v-2h-3v-2h3v-2h-2v-3h2v-2h2V9Z"
            />

            {/* Deep Charcoal Obsidian Fur Base */}
            <path
              className="midnight-sprite-lab__cat-body"
              d="M18 10h4v3h8v-3h4v6h1v2h2v-2h2v3h-2v2h3v2h-3v2h2v3h-2v2h-1v2h2v4h-2v3h-2v2h-3v1H20v-1h-3v-2h-2v-3h-2v-4h2v-2h-1v-2h-2v-3h2v-2h-3v-2h3v-2h-2v-3h2v2h2v-2h1v-6h3Z"
            />

            {/* Cool Slate-Blue Sheen Highlights */}
            <path
              className="midnight-sprite-lab__cat-sheen"
              d="M21 15h10v2h-10ZM16 22h3v3h-3ZM33 22h3v3h-3ZM17 31h4v4h-4ZM31 31h4v4h-3Z"
            />

            {/* Shading Under Belly & Cheeks */}
            <path
              className="midnight-sprite-lab__cat-shadow"
              d="M16 39h5v2h-5ZM31 39h5v2h-5ZM21 40h10v1H21Z"
            />

            {/* Pink Inner Ears */}
            <path
              className="midnight-sprite-lab__inner-ear"
              d="M19 13h2v3h-2ZM31 13h2v3h-2Z"
            />

            {/* Clean White V-Shaped Chest Bib */}
            <path
              className="midnight-sprite-lab__cat-white"
              d="M24 27h4v2h2v3h-2v3h-2v2h-2v-2h-2v-3h-2v-3h2v-2h2Z"
            />

            {/* White Muzzle around Nose */}
            <path
              className="midnight-sprite-lab__cat-white"
              d="M24 22h4v3h-4ZM22 24h2v2h-2ZM28 24h2v2h-2Z"
            />

            {/* Cozy Knitted Midnight Scarf around Neck */}
            <g className="midnight-sprite-lab__scarf-group">
              {/* Scarf Collar Band */}
              <path
                className="midnight-sprite-lab__scarf-main"
                d="M20 28h12v3H20Z"
              />
              {/* Scarf Stripes */}
              <path
                className="midnight-sprite-lab__scarf-stripe"
                d="M22 28h2v3h-2ZM26 28h2v3h-2ZM30 28h2v3h-2Z"
              />
              {/* Hanging Scarf Tails */}
              <path
                className="midnight-sprite-lab__scarf-tail"
                d="M20 31h4v4h-4Z"
              />
              <path
                className="midnight-sprite-lab__scarf-fringe"
                d="M20 35h4v1h-4Z"
              />
            </g>

            {/* Neat Front White Paws (Side by Side with crisp division!) */}
            <path
              className="midnight-sprite-lab__cat-white"
              d="M21 39h4v2h-4ZM27 39h4v2h-4Z"
            />

            {/* Soft Warm Pink Cheek Blush */}
            <path
              className="midnight-sprite-lab__blush"
              d="M17 26h3v2h-3ZM32 26h3v2h-3Z"
            />

            {/* Eyes */}
            {/* Idle Sleepy Eyes (Cute clean bold straight lines - -) */}
            <path
              className="midnight-sprite-lab__eye--sleep"
              d="M18 21h4v2h-4ZM30 21h4v2h-4Z"
            />

            {/* Awake Eyes (Kawaii Sparkling Celestial Cyan Anime Eyes) */}
            <g className="midnight-sprite-lab__eye-awake-group">
              <path
                className="midnight-sprite-lab__eye-contour"
                d="M18 19h5v5h-5ZM29 19h5v5h-5Z"
              />
              <path
                className="midnight-sprite-lab__eye--awake"
                d="M19 20h3v3h-3ZM30 20h3v3h-3Z"
              />
              <path
                className="midnight-sprite-lab__eye-pupil"
                d="M20 21h2v2h-2ZM31 21h2v2h-2Z"
              />
              <path
                className="midnight-sprite-lab__eye-glint"
                d="M19 20h1v1h-1ZM30 20h1v1h-1ZM21 22h1v1h-1ZM32 22h1v1h-1Z"
              />
            </g>

            {/* Cute Dark Nose & Mouth */}
            <path className="midnight-sprite-lab__nose" d="M25 23h2v2h-2Z" />
            <path
              className="midnight-sprite-lab__mouth"
              d="M24 25h1v1h-1ZM27 25h1v1h-1Z"
            />
          </g>
        </g>

        {/* State Indicators */}
        <path className="midnight-sprite-lab__scan-spark" d="M41 16h1v2h2v1h-2v2h-1v-2h-2v-1h2Z" />
        <path className="midnight-sprite-lab__alarm-mark" d="M6 22h2v5H6Zm0 7h2v2H6Z" />
      </g>
    </svg>
  );
}
