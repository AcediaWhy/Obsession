import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./FallenSpriteLab.css";

export type FallenDetail = "auto" | "base" | "hero";

export type FallenSpriteLabProps = {
  phase?: ObsessionVisualPhase;
  detail?: FallenDetail;
  paused?: boolean;
  size?: number | string;
};

type Mood = "idle" | "busy" | "scanning" | "active" | "alarm";
type PixelToken = "#" | "W" | "G" | "B" | "C";

// Exact 64×52 logical-pixel trace of the user-supplied canonical Temmie reference.
// The facial pixels are intentionally left blank and layered below so states can
// change expression without distorting Temmie's silhouette or proportions.
const TEMMIE_REFERENCE_ROWS = [
  "",
  "               ###",
  "              ######                 ###",
  "              #######     ## #     ######",
  "              ########   ####     #######",
  "              ###G##### ####     ########",
  "             ####GG############ #####G###",
  "             ###GGGG################GGG##",
  "             ###GG##################GGG##",
  "             #########################G###",
  "            ###############################",
  "           ################################",
  "           ############W####################",
  "          ############WWW###################",
  "          ###########WWWW###################",
  "          ##########WWWWWW##################",
  "          #########WWWWWWWW##################",
  "         #########WWWWWWWWWW#################",
  "         ########WWWWWWWWWWWW################",
  "         #######WW   WWWWWWWWWWWW############",
  "       #########WW   WWWWWWWW   W############   ###",
  "      #WW######WWW   WWWWWWWW   W#####W#########WWW#",
  "     #WWW######WWWWWWWWWWWWWW   WW####W#####WWWWWWW#",
  "     #WWW####W#WWWWWWWWW WWWWWWWWW####W####WWWWWWWW#",
  "     #WWWW##W##WWWWWWWWWWWWWWWWWWW####W####WWWWWWWW#",
  "     #WWWWW####WWWWW WWWWWWW WWWWW####W###WWWWWWWWW#",
  "     #WWWWW####WWWWW WWW WWW WWWWW####W###WWWWWWWW#",
  "     #WWWWW#####WWWWW   W   WWWWWW####W####WWWW###",
  "     #WWWWW#####WWWWWWWWWWWWWWWWWW###W#########     #####",
  "      #WWWW######WWWWWWWWWWWWWWWWW###W#########   ##WWWW#",
  "       #WWW######WWWWWWWWWWWWWWWW####W#########  #WWWWWW#",
  "        ##########WWWWWWWWWWWWWW#####W###########WWWWWWW#",
  "           #########WWWWWWWWWWW#########WWWW##WWWWWWWWW#",
  "           ########B###########B######C#WWWWWWWWWWWW###",
  "           ###### ##BBBBBBBBBBB#######CC#WWWWWWWWW##",
  "           #####  #C###########CC#####C##WWWWWWWW#",
  "            ###   #CCCCCCCCCCCCCC####CCC#WWWWWWWWW#",
  "            ###   #CCCCCCCCCCCCCCC###CCC#WWWWWWWWW#",
  "             #   #CCCCC#CCCCCCCCCCC#CCCC#WWWWWWWWWW#",
  "                 #CCCC# #CCCCCC#CCCCCCCC#WWWWWWWWWW#",
  "                 #CCCC# ##CCCC# #CCCCCC#WWWW#WWWWWW#",
  "                 #CCCC#   #CCC# #CCCCCC#WWWW#WWWWWW#",
  "                 #CCCC#   #CCC# ##CCCC##WWWW#WWWWWW#",
  "                 ######   #CCC#   #####WWW###WWWWWW#",
  "                 #WWWW#   #####      #####  #WWWWW#",
  "                 #WWWW#   #WWW#     #WWWW#  #WWWWW#",
  "                 #WWWW#   #WWW#     #WWWW# #WWWWWW#",
  "                  ####     ###     #WWWW# #WWWWWW#",
  "                                   #####  #######",
  "",
  "",
  "",
] as const;

function pixelPath(token: PixelToken): string {
  let path = "";

  TEMMIE_REFERENCE_ROWS.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      if (row[x] !== token) {
        x += 1;
        continue;
      }

      const start = x;
      while (row[x] === token) x += 1;
      const width = x - start;
      path += `M${start} ${y}h${width}v1h-${width}Z`;
    }
  });

  return path;
}

const TEMMIE_REFERENCE_PATHS = {
  black: pixelPath("#"),
  white: pixelPath("W"),
  grey: pixelPath("G"),
  blue: pixelPath("B"),
  cyan: pixelPath("C"),
} as const;

// Exact face pixels from the reference: two uneven button eyes, a single-pixel
// nose and Temmie's characteristic split :3 smile. Mood lives in motion, not
// in replacement eyes or mouths that make the character stop looking canonical.
const CANONICAL_FACE = [
  "M18 19h3v3h-3Z",
  "M29 20h3v3h-3Z",
  "M24 23h1v1h-1Z",
  "M20 25h1v1h-1ZM28 25h1v1h-1Z",
  "M20 26h1v1h-1ZM24 26h1v1h-1ZM28 26h1v1h-1Z",
  "M21 27h3v1h-3ZM25 27h3v1h-3Z",
].join("");

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

function resolveDetail(detail: FallenDetail, size: number | string): "base" | "hero" {
  if (detail === "base" || detail === "hero") return detail;
  const numeric = typeof size === "number" ? size : Number.parseFloat(size);
  return Number.isFinite(numeric) && numeric <= 120 ? "base" : "hero";
}

export function FallenSpriteLab({
  phase = "idle",
  detail = "auto",
  paused = false,
  size = 240,
}: FallenSpriteLabProps) {
  const mood = moodForPhase(phase);
  const resolvedDetail = resolveDetail(detail, size);
  const motion = paused ? "paused" : "running";
  const sizeValue = typeof size === "number" ? `${size}px` : size;

  return (
    <svg
      aria-label="Temmie in the canonical Undertale pixel-art silhouette with four ears, black hair, blue shirt and raised tail"
      className="fallen-sprite-lab"
      data-detail={resolvedDetail}
      data-fallen-sprite-lab
      data-mood={mood}
      data-motion={motion}
      data-phase={phase}
      data-reference-sprite="temmie"
      preserveAspectRatio="xMidYMid meet"
      role="img"
      shapeRendering="crispEdges"
      style={{ ["--fallen-core-size" as string]: sizeValue }}
      viewBox="0 0 64 52"
    >
      <g className={`fallen-sprite-lab__scene fallen-sprite-lab__scene--${resolvedDetail}`}>
        <g className="fallen-sprite-lab__hero-detail">
          <path
            className="fallen-sprite-lab__core-backlight"
            d="M10 8h34v2h6v5h4v24h-4v5h-8v4H14v-3H8v-6H5V17h3v-6h2Z"
          />
          <g className="fallen-sprite-lab__cave-stars">
            <path className="fallen-sprite-lab__star" d="M3 12h1v3H3ZM2 13h3v1H2ZM60 17h1v3h-1ZM59 18h3v1H59Z" />
            <path className="fallen-sprite-lab__sparkle" d="M4 39h1v2H4ZM3 40h3v1H3ZM59 45h1v2h-1ZM58 46h3v1H58Z" />
          </g>

          <g className="fallen-sprite-lab__soul-halo-group">
            <path className="fallen-sprite-lab__soul-halo" d="M47 1h13v2h2v8h-2v2H49v-2h-2Z" />
          </g>
          <g className="fallen-sprite-lab__soul-heart-group">
            <path className="fallen-sprite-lab__tem-outline" d="M49 3h4v1h1V3h4v6h-1v1h-1v1h-1v1h-1v1h-1v-1h-1v-1h-1v-1h-1V9h-1Z" />
            <path className="fallen-sprite-lab__soul-red" d="M50 4h3v1h1V4h3v4h-1v1h-1v1h-1v1h-1v-1h-1V9h-1V8h-1Z" />
            <path className="fallen-sprite-lab__soul-glint" d="M50 4h1v2h-1ZM51 4h1v1h-1Z" />
          </g>

          <g className="fallen-sprite-lab__dream-zzz">
            <path className="fallen-sprite-lab__zzz-outline" d="M52 14h6v2h-2v1h2v2h-7v-2h2v-1h-1Z" />
            <path className="fallen-sprite-lab__zzz-core" d="M53 15h4v1h-2v1h2v1h-5v-1h2v-1h-1Z" />
          </g>
          <g className="fallen-sprite-lab__active-sparks">
            <path className="fallen-sprite-lab__save-star-outline" d="M57 13h3v1h1v3h-1v1h-3v-1h-1v-3h1Z" />
            <path className="fallen-sprite-lab__save-star-core" d="M58 14h1v3h-1ZM57 15h3v1h-3Z" />
            <path className="fallen-sprite-lab__active-spark" d="M3 17h1v3H3ZM2 18h3v1H2ZM60 34h1v3h-1ZM59 35h3v1H59Z" />
          </g>
        </g>

        <g className="fallen-sprite-lab__temmie">
          <path className="fallen-sprite-lab__reference-black" d={TEMMIE_REFERENCE_PATHS.black} />
          <path className="fallen-sprite-lab__reference-white" d={TEMMIE_REFERENCE_PATHS.white} />
          <path className="fallen-sprite-lab__reference-grey" d={TEMMIE_REFERENCE_PATHS.grey} />
          <path className="fallen-sprite-lab__reference-blue" d={TEMMIE_REFERENCE_PATHS.blue} />
          <path className="fallen-sprite-lab__reference-cyan" d={TEMMIE_REFERENCE_PATHS.cyan} />

          <g className="fallen-sprite-lab__head-group">
            <path className="fallen-sprite-lab__tem-outline fallen-sprite-lab__tem-face" d={CANONICAL_FACE} />
          </g>
        </g>

        <path className="fallen-sprite-lab__scan-spark" d="M59 22h1v2h2v1h-2v2h-1v-2h-2v-1h2Z" />
        <path className="fallen-sprite-lab__alarm-mark" d="M2 24h2v6H2Zm0 8h2v2H2Z" />
      </g>
    </svg>
  );
}
