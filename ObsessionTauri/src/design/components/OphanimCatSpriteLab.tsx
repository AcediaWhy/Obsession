import type { CSSProperties } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./OphanimCatSpriteLab.css";

export type OphanimCatDetail = "auto" | "base" | "hero";

export type OphanimCatSpriteLabProps = {
  phase?: ObsessionVisualPhase;
  detail?: OphanimCatDetail;
  paused?: boolean;
  size?: number | string;
};

type Mood = "idle" | "busy" | "scanning" | "active" | "alarm";

const RING_EYES = [
  { x: 24, y: 4, detail: false },
  { x: 38, y: 8, detail: true },
  { x: 46, y: 18, detail: false },
  { x: 45, y: 34, detail: true },
  { x: 36, y: 45, detail: false },
  { x: 12, y: 41, detail: true },
  { x: 4, y: 29, detail: false },
  { x: 7, y: 14, detail: true },
] as const;

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

function resolveDetail(detail: OphanimCatDetail, size: number | string): "base" | "hero" {
  if (detail === "base" || detail === "hero") return detail;
  const numeric = typeof size === "number" ? size : Number.parseFloat(size);
  return Number.isFinite(numeric) && numeric <= 120 ? "base" : "hero";
}

export function OphanimCatSpriteLab({
  phase = "idle",
  detail = "auto",
  paused = false,
  size = 260,
}: OphanimCatSpriteLabProps) {
  const mood = moodForPhase(phase);
  const resolvedDetail = resolveDetail(detail, size);
  const sizeValue = typeof size === "number" ? `${size}px` : size;

  return (
    <svg
      aria-label="Pixel-art guardian cat at the hub of eye-covered Ophanim wheels"
      className="ophanim-cat-sprite-lab"
      data-detail={resolvedDetail}
      data-mood={mood}
      data-motion={paused ? "paused" : "running"}
      data-ophanim-cat-sprite-lab
      data-phase={phase}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      shapeRendering="crispEdges"
      style={{ ["--ophanim-cat-size" as string]: sizeValue }}
      viewBox="0 0 52 52"
    >
      <g className={`ophanim-cat-sprite-lab__scene ophanim-cat-sprite-lab__scene--${resolvedDetail}`}>
        <g className="ophanim-cat-sprite-lab__glory-rays">
          <rect x="25" y="0" width="2" height="4" />
          <rect x="25" y="48" width="2" height="4" />
          <rect x="0" y="25" width="4" height="2" />
          <rect x="48" y="25" width="4" height="2" />
          <rect x="6" y="6" width="3" height="2" />
          <rect x="43" y="6" width="3" height="2" />
          <rect x="6" y="44" width="3" height="2" />
          <rect x="43" y="44" width="3" height="2" />
        </g>

        <path className="ophanim-cat-sprite-lab__halo" d="M18 3H34V5H40V8H44V12H47V18H49V34H47V40H44V44H40V47H34V49H18V47H12V44H8V40H5V34H3V18H5V12H8V8H12V5H18Z" />

        <g className="ophanim-cat-sprite-lab__wheels">
          <path className="ophanim-cat-sprite-lab__wheel ophanim-cat-sprite-lab__wheel--outer" d="M10 12H16V8H36V10H42V14H46V20H49V32H46V38H42V42H36V45H16V43H10V39H6V33H3V21H6V15H10Z" />
          <path className="ophanim-cat-sprite-lab__wheel ophanim-cat-sprite-lab__wheel--cross" d="M24 4H29V7H33V11H37V15H41V19H45V23H49V28H45V32H41V36H37V40H33V44H29V48H24V45H20V41H16V37H12V33H8V29H4V24H8V20H12V16H16V12H20V8H24Z" />
          <path className="ophanim-cat-sprite-lab__wheel ophanim-cat-sprite-lab__wheel--inner" d="M14 21H18V17H34V19H39V23H42V31H39V35H34V38H18V36H14V32H11V25H14Z" />
        </g>

        <g className="ophanim-cat-sprite-lab__ring-eyes">
          {RING_EYES.map((eye, index) => (
            <g className={eye.detail ? "ophanim-cat-sprite-lab__detail-eye" : undefined} key={`${eye.x}-${eye.y}`} transform={`translate(${eye.x} ${eye.y})`}>
              <g className="ophanim-cat-sprite-lab__ring-eye" style={{ ["--ophanim-eye-delay" as string]: `${index * 90}ms` } as CSSProperties}>
                <rect className="ophanim-cat-sprite-lab__ring-eye-lid" x="-2" y="1" width="5" height="1" />
                <path className="ophanim-cat-sprite-lab__ring-eye-socket" d="M-2 0H3V3H-2Z" />
                <rect className="ophanim-cat-sprite-lab__ring-eye-pupil" x="0" y="1" width="1" height="1" />
              </g>
            </g>
          ))}
        </g>

        <g className="ophanim-cat-sprite-lab__cat">
          <path className="ophanim-cat-sprite-lab__tail-ring" d="M33 36H40V38H44V41H47V47H45V50H38V48H43V46H44V42H41V40H35" />
          <path className="ophanim-cat-sprite-lab__body" d="M18 33H34V35H38V39H41V45H38V48H18V46H15V39H17V35H18Z" />
          <path className="ophanim-cat-sprite-lab__belly" d="M21 37H32V39H35V45H32V47H21V45H19V39H21Z" />
          <path className="ophanim-cat-sprite-lab__head" d="M17 20H18V12H21V14H23V17H29V16H31V14H34V12H37V20H38V30H36V33H33V35H21V33H18V30H16V22H17Z" />
          <path className="ophanim-cat-sprite-lab__ear-inlay" d="M19 15H21V17H23V20H19ZM34 15H36V20H32V17H34Z" />

          <g className="ophanim-cat-sprite-lab__sleep-eyes">
            <rect x="21" y="26" width="4" height="1" />
            <rect x="30" y="26" width="4" height="1" />
          </g>
          <g className="ophanim-cat-sprite-lab__muzzle">
            <rect x="27" y="28" width="2" height="1" />
            <rect x="26" y="29" width="1" height="2" />
            <rect x="29" y="29" width="1" height="2" />
            <rect x="24" y="30" width="2" height="1" />
            <rect x="30" y="30" width="2" height="1" />
          </g>

          <g className="ophanim-cat-sprite-lab__third-eye">
            <path className="ophanim-cat-sprite-lab__third-eye-socket" d="M22 22V20H24V19H31V20H33V22H31V24H24V23H22Z" />
            <rect className="ophanim-cat-sprite-lab__third-eye-iris" x="26" y="20" width="3" height="3" />
            <rect className="ophanim-cat-sprite-lab__third-eye-pupil" x="27" y="20" width="1" height="3" />
            <rect className="ophanim-cat-sprite-lab__third-eye-glint" x="26" y="20" width="1" height="1" />
          </g>

          <g className="ophanim-cat-sprite-lab__whiskers">
            <rect x="11" y="29" width="6" height="1" />
            <rect x="13" y="32" width="5" height="1" />
            <rect x="38" y="29" width="6" height="1" />
            <rect x="37" y="32" width="5" height="1" />
          </g>
          <g className="ophanim-cat-sprite-lab__paws">
            <rect x="20" y="44" width="5" height="4" />
            <rect x="31" y="44" width="5" height="4" />
          </g>
        </g>

        <g className="ophanim-cat-sprite-lab__scan-reticle">
          <rect x="27" y="8" width="1" height="8" />
          <rect x="27" y="27" width="1" height="7" />
          <rect x="14" y="21" width="7" height="1" />
          <rect x="34" y="21" width="8" height="1" />
          <path className="ophanim-cat-sprite-lab__scan-beam" d="M29 20H34V18H39V16H44V14H49" />
        </g>

        <path className="ophanim-cat-sprite-lab__alarm-ring" d="M9 8H17V5H35V7H42V11H46V17H49V35H46V41H42V45H35V48H17V46H10V42H6V36H3V18H6V12H9Z" />
      </g>
    </svg>
  );
}
