import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./CatnapSpriteLab.css";

export type CatnapDetail = "auto" | "base" | "hero";

export type CatnapSpriteLabProps = {
  phase?: ObsessionVisualPhase;
  detail?: CatnapDetail;
  paused?: boolean;
  size?: number | string;
};

type Mood = "idle" | "busy" | "scanning" | "active" | "alarm";

function moodForPhase(phase: ObsessionVisualPhase): Mood {
  switch (phase) {
    case "engaging":
      return "busy";
    case "scanning":
      return "scanning";
    case "focused":
      return "active";
    case "fault":
      return "alarm";
    case "idle":
    default:
      return "idle";
  }
}

function resolveDetail(detail: CatnapDetail, size?: number | string): "base" | "hero" {
  if (detail !== "auto") return detail;
  if (typeof size === "number") return size >= 160 ? "hero" : "base";
  return "hero";
}

export function CatnapSpriteLab({
  phase = "idle",
  detail = "auto",
  paused = false,
  size = 240,
}: CatnapSpriteLabProps) {
  const mood = moodForPhase(phase);
  const resolvedDetail = resolveDetail(detail, size);
  const motion = paused ? "paused" : "running";
  const sizeValue = typeof size === "number" ? `${size}px` : size;

  return (
    <svg
      aria-label="Catnap chubby fluffy orange tabby cat pixel art sprite"
      className="catnap-sprite-lab"
      data-catnap-sprite-lab
      data-detail={resolvedDetail}
      data-mood={mood}
      data-motion={motion}
      data-phase={phase}
      role="img"
      shapeRendering="crispEdges"
      style={{ ["--catnap-cat-size" as string]: sizeValue }}
      viewBox="0 0 52 52"
    >
      <g className={`catnap-sprite-lab__scene catnap-sprite-lab__scene--${resolvedDetail}`}>
        {/* Ambient Warm Sunset Dust Motes */}
        <g className="catnap-sprite-lab__motes">
          <path className="catnap-sprite-lab__mote catnap-sprite-lab__mote--1" d="M8 18h1v1H8ZM45 16h1v1h-1ZM23 4h1v1h-1ZM10 32h1v1h-1Z" />
          <path className="catnap-sprite-lab__mote catnap-sprite-lab__mote--2" d="M42 6h1v1h-1ZM13 10h1v1h-1ZM46 28h1v1h-1ZM29 8h1v1h-1Z" />
        </g>

        {/* Soft Organic Ground & Paw Contact Shadows */}
        <g className="catnap-sprite-lab__ground-shadow-group">
          <path
            className="catnap-sprite-lab__ground-shadow"
            d="M17 41h18v1H17ZM36 39h7v2h-7ZM19 42h14v1H19Z"
          />
          <path
            className="catnap-sprite-lab__ground-ambient"
            d="M14 42h24v1H14ZM18 43h16v1H18Z"
          />
        </g>

        {/* Perfect Pixel-Art "Z Z Z" Sleeping Dreams (Visible in Idle) */}
        <g className="catnap-sprite-lab__dream-zzz">
          {/* Small Z (3x4) */}
          <g className="catnap-sprite-lab__zzz-group catnap-sprite-lab__zzz-group--1">
            <path
              className="catnap-sprite-lab__zzz-outline"
              d="M34 16h5v2h-1v1h2v2h-6v-2h2v-1h-2v-2Z"
            />
            <path
              className="catnap-sprite-lab__zzz-core"
              d="M35 17h3v1h-3ZM36 18h2v1h-2ZM35 19h3v1h-3Z"
            />
          </g>

          {/* Mid Z (4x5) */}
          <g className="catnap-sprite-lab__zzz-group catnap-sprite-lab__zzz-group--2">
            <path
              className="catnap-sprite-lab__zzz-outline"
              d="M38 10h6v2h-2v1h-1v-1h-1v1h3v2h-7v-2h3v-1h-1v-1h-2v-2Z"
            />
            <path
              className="catnap-sprite-lab__zzz-core"
              d="M39 11h4v1h-4ZM41 12h2v1h-2ZM40 13h2v1h-2ZM39 14h4v1h-4Z"
            />
          </g>

          {/* Big Z (5x6) */}
          <g className="catnap-sprite-lab__zzz-group catnap-sprite-lab__zzz-group--3">
            <path
              className="catnap-sprite-lab__zzz-outline"
              d="M43 3h7v2h-3v1h-1v-1h-1v1h1v1h3v2h-8v-2h3v-1h-1v-1h-1v-1h-2v-2Z"
            />
            <path
              className="catnap-sprite-lab__zzz-core"
              d="M44 4h5v1h-5ZM47 5h2v1h-2ZM46 6h2v1h-2ZM45 7h2v1h-2ZM44 8h5v1h-5Z"
            />
          </g>

          {/* Golden Dream Spark */}
          <path
            className="catnap-sprite-lab__dream-star-outline"
            d="M31 7h3v1h1v3h-1v1h-3v-1h-1V8h1Z"
          />
          <path
            className="catnap-sprite-lab__dream-star-core"
            d="M32 8h1v3h-1ZM31 9h3v1h-3Z"
          />
        </g>

        {/* Active Mode: Dancing Sunset Sparkles & Heart Charm */}
        <g className="catnap-sprite-lab__active-sparks">
          <path
            className="catnap-sprite-lab__active-spark"
            d="M36 10h1v3h-1ZM35 11h3v1h-3ZM44 6h1v3h-1ZM43 7h3v1h-3ZM42 16h1v2h-1ZM41 17h3v1h-3Z"
          />
        </g>

        {/* Reference 1: Layered Chubby Fluffy Orange Tabby Cat */}
        <g className="catnap-sprite-lab__cat">
          {/* 1. Tail Group (Curled on Right) */}
          <g className="catnap-sprite-lab__tail-group">
            {/* Outline */}
            <path
              className="catnap-sprite-lab__cat-outline"
              d="M36 32h5v-3h2v-3h4v8h-2v3h-2v2h-7v-2h2v-2h-2Z"
            />
            {/* Orange Base */}
            <path
              className="catnap-sprite-lab__cat-fur-orange"
              d="M37 33h4v-3h2v-2h2v6h-2v3h-2v1h-4Z"
            />
            {/* Auburn Stripes */}
            <path
              className="catnap-sprite-lab__cat-stripes"
              d="M40 30h2v2h-2ZM38 35h3v2h-3Z"
            />
            {/* White Tip */}
            <path
              className="catnap-sprite-lab__cat-fur-white"
              d="M42 27h2v2h-2Z"
            />
          </g>

          {/* 2. Main Body & Face Structure */}
          <g className="catnap-sprite-lab__body">
            {/* 1px Dark Contour Rim behind the fur */}
            <path
              className="catnap-sprite-lab__cat-outline"
              d="M17 9h6v3h6V9h6v7h2v2h2v3h-2v2h3v2h-3v2h2v3h-2v2h-1v2h2v4h-2v3h-2v2h-3v2H19v-2h-3v-2h-2v-3h-2v-4h2v-2h-1v-2h-2v-3h2v-2h-3v-2h3v-2h-2v-3h2v-2h2V9Z"
            />

            {/* Vibrant Rich Golden-Orange Fur Base (Reference 1) */}
            <path
              className="catnap-sprite-lab__cat-fur-orange"
              d="M18 10h4v3h8v-3h4v6h1v2h2v-2h2v3h-2v2h3v2h-3v2h2v3h-2v2h-1v2h2v4h-2v3h-2v2h-3v1H20v-1h-3v-2h-2v-3h-2v-4h2v-2h-1v-2h-2v-3h2v-2h-3v-2h3v-2h-2v-3h2v2h2v-2h1v-6h3Z"
            />

            {/* Sunlight Peach/Gold Highlights (Soft, Natural) */}
            <path
              className="catnap-sprite-lab__cat-fur-light"
              d="M21 15h10v2h-10ZM16 22h3v3h-3ZM33 22h3v3h-3ZM17 31h4v4h-4ZM31 31h4v4h-3Z"
            />

            {/* Dark Cinnamon / Auburn Tabby Stripes (Reference 1) */}
            <path
              className="catnap-sprite-lab__cat-stripes"
              d="M25 13h2v4h-2ZM21 16h2v3h-2ZM29 16h2v3h-2ZM16 33h4v2h-4ZM32 33h4v2h-4ZM17 37h3v2h-3ZM32 37h3v2h-3Z"
            />

            {/* Warm Deep Shading Under Belly & Cheeks */}
            <path
              className="catnap-sprite-lab__cat-shadow"
              d="M16 39h5v2h-5ZM31 39h5v2h-5ZM21 40h10v1H21Z"
            />

            {/* Pink Inner Ears */}
            <path
              className="catnap-sprite-lab__inner-ear"
              d="M19 13h2v3h-2ZM31 13h2v3h-2Z"
            />

            {/* Clean White V-Shaped Chest Bib (Reference 1) */}
            <path
              className="catnap-sprite-lab__cat-fur-white"
              d="M24 27h4v2h2v3h-2v3h-2v2h-2v-2h-2v-3h-2v-3h2v-2h2Z"
            />

            {/* White Muzzle around Nose */}
            <path
              className="catnap-sprite-lab__cat-fur-white"
              d="M24 22h4v3h-4ZM22 24h2v2h-2ZM28 24h2v2h-2Z"
            />

            {/* Neat Front White Paws */}
            <path
              className="catnap-sprite-lab__cat-fur-white"
              d="M21 39h4v2h-4ZM27 39h4v2h-4Z"
            />

            {/* Soft Warm Pink Cheek Blush */}
            <path
              className="catnap-sprite-lab__blush"
              d="M17 26h3v2h-3ZM32 26h3v2h-3Z"
            />

            {/* Eyes */}
            {/* Idle Sleepy Eyes (Cute clean bold straight lines - -) */}
            <path
              className="catnap-sprite-lab__eye--sleep"
              d="M18 21h4v2h-4ZM30 21h4v2h-4Z"
            />

            {/* Awake Eyes (Kawaii Sparkling Honey Anime Eyes with Double Speculars) */}
            <g className="catnap-sprite-lab__eye-awake-group">
              {/* Dark Outline */}
              <path
                className="catnap-sprite-lab__eye-contour"
                d="M18 19h5v5h-5ZM29 19h5v5h-5Z"
              />
              {/* Amber Iris */}
              <path
                className="catnap-sprite-lab__eye--awake"
                d="M19 20h3v3h-3ZM30 20h3v3h-3Z"
              />
              {/* Deep Pupil */}
              <path
                className="catnap-sprite-lab__eye-pupil"
                d="M20 21h2v2h-2ZM31 21h2v2h-2Z"
              />
              {/* Double Specular Highlights */}
              <path
                className="catnap-sprite-lab__eye-glint"
                d="M19 20h1v1h-1ZM30 20h1v1h-1ZM21 22h1v1h-1ZM32 22h1v1h-1Z"
              />
            </g>

            {/* Cute Dark Nose & Muzzle */}
            <path className="catnap-sprite-lab__nose" d="M25 23h2v2h-2Z" />
            <path
              className="catnap-sprite-lab__mouth"
              d="M24 25h1v1h-1ZM27 25h1v1h-1Z"
            />
          </g>
        </g>

        {/* State Indicators */}
        <path className="catnap-sprite-lab__scan-spark" d="M41 16h1v2h2v1h-2v2h-1v-2h-2v-1h2Z" />
        <path className="catnap-sprite-lab__alarm-mark" d="M6 22h2v5H6Zm0 7h2v2H6Z" />
      </g>
    </svg>
  );
}
