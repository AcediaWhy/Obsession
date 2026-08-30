import { useId } from "react";
import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./RainUmbrellaSpriteLab.css";

export type RainUmbrellaMood = "idle" | "busy" | "scanning" | "active" | "alarm";
export type RainUmbrellaDetail = "auto" | "base" | "hero";

export function rainUmbrellaMoodForPhase(phase: ObsessionVisualPhase): RainUmbrellaMood {
  if (phase === "fault") return "alarm";
  if (phase === "focused") return "active";
  if (phase === "engaging") return "busy";
  return phase;
}

type RainUmbrellaSpriteLabProps = {
  phase: ObsessionVisualPhase;
  detail?: RainUmbrellaDetail;
  paused?: boolean;
  size?: number;
};

export function RainUmbrellaSpriteLab({
  phase,
  detail = "auto",
  paused = false,
  size = 240,
}: RainUmbrellaSpriteLabProps) {
  const mood = rainUmbrellaMoodForPhase(phase);
  const resolvedDetail = size >= 180 && detail !== "base" ? "hero" : "base";
  const hero = resolvedDetail === "hero";
  const patternKey = useId().replace(/:/g, "");
  const rainAId = `${patternKey}-umbrella-rain-a`;
  const rainBId = `${patternKey}-umbrella-rain-b`;

  return (
    <svg
      aria-hidden="true"
      className="rain-umbrella-sprite-lab"
      data-detail={resolvedDetail}
      data-mood={mood}
      data-motion={paused ? "still" : "running"}
      data-rain-umbrella-sprite
      focusable="false"
      shapeRendering="crispEdges"
      style={{ width: size, height: size }}
      viewBox={hero ? "0 0 120 120" : "0 0 52 52"}
    >
      {hero ? (
        <g className="rain-umbrella-sprite-lab__scene rain-umbrella-sprite-lab__scene--hero">
          <defs>
            <pattern id={rainAId} width="120" height="48" patternUnits="userSpaceOnUse">
              <path className="rain-umbrella-sprite-lab__rain-drop" d="M8 3h1v8H8ZM26 35h1v6h-1ZM102 17h1v7h-1Z" />
            </pattern>
            <pattern id={rainBId} width="120" height="48" patternUnits="userSpaceOnUse">
              <path className="rain-umbrella-sprite-lab__rain-drop" d="M34 12h1v7h-1ZM75 38h1v6h-1ZM111 5h1v8h-1Z" />
            </pattern>
          </defs>

          <g className="rain-umbrella-sprite-lab__stars">
            <path d="M18 13h2v2h-2ZM88 9h2v2h-2ZM108 30h1v2h-1Z" />
          </g>
          <rect className="rain-umbrella-sprite-lab__rain-field rain-umbrella-sprite-lab__rain-field--a" x="3" y="-48" width="114" height="216" fill={`url(#${rainAId})`} />
          <rect className="rain-umbrella-sprite-lab__rain-field rain-umbrella-sprite-lab__rain-field--b" x="3" y="-48" width="114" height="216" fill={`url(#${rainBId})`} />

          <g className="rain-umbrella-sprite-lab__puddle">
            <path className="rain-umbrella-sprite-lab__puddle-outline" d="M28 101h15v-2h32v2h13v3h8v6h-6v4H76v3H42v-2H29v-3H21v-7h7Z" />
            <path className="rain-umbrella-sprite-lab__puddle-mid" d="M31 104h14v-2h28v2h14v2h6v3h-8v3H73v2H43v-2H30v-2h-6v-4h7Z" />
            <path className="rain-umbrella-sprite-lab__puddle-light" d="M38 105h18v-1h19v2h8v2h-14v2H42v-1H31v-2h7Z" />
            <path className="rain-umbrella-sprite-lab__puddle-warm" d="M58 110h19v2H58Z" />
          </g>

          <g className="rain-umbrella-sprite-lab__umbrella-rig">
            <g className="rain-umbrella-sprite-lab__umbrella">
              <path className="rain-umbrella-sprite-lab__umbrella-outline" d="M13 37h3v-7h6v-6h8v-5h10v-4h40v4h10v5h8v6h6v9h3v10H89v4H77v-4H65v5H53v-5H41v4H29v-4H13Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-fabric" d="M18 36h3v-6h7v-5h9v-4h10v-3h26v3h10v4h8v5h6v7h3v9H88v3H77v-4H65v4H53v-4H41v3H29v-3H18Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-panel rain-umbrella-sprite-lab__umbrella-panel--light" d="M47 18h11v27h-5v-4H41v3H30v-3H22v-6h3v-6h6v-5h8v-3h8Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-panel rain-umbrella-sprite-lab__umbrella-panel--dark" d="M60 18h13v3h10v4h8v5h6v7h3v8H88v4H77v-4H65v4h-5Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-ribs" d="M58 17h3v32h-3ZM39 21h2v4h2v4h2v5h2v11h-2V35h-2v-5h-2v-4h-2ZM79 22h2v4h2v4h2v5h2v10h-2v-9h-2v-5h-2v-4h-2Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-glint" d="M30 27h9v2h-9ZM25 31h8v1h-8ZM66 23h8v1h-8Z" />
            </g>

            <g className="rain-umbrella-sprite-lab__handle">
              <path className="rain-umbrella-sprite-lab__handle-outline" d="M75 46h6v48h4v10h-3v5H72v-3h-4v-9h6v6h4v-3h1v-5h-4Z" />
              <path className="rain-umbrella-sprite-lab__handle-metal" d="M77 48h2v46h-2ZM79 96h3v7h-2v3h-6v-2h5Z" />
              <path className="rain-umbrella-sprite-lab__handle-glint" d="M77 50h1v28h-1Z" />
            </g>
          </g>

          <path className="rain-umbrella-sprite-lab__drip rain-umbrella-sprite-lab__drip--left" d="M14 50h2v6h-2Z" />
          <path className="rain-umbrella-sprite-lab__drip rain-umbrella-sprite-lab__drip--right" d="M104 50h2v7h-2Z" />
          <path className="rain-umbrella-sprite-lab__drip rain-umbrella-sprite-lab__drip--middle" d="M64 54h1v5h-1Z" />

          <g className="rain-umbrella-sprite-lab__cat-shadow">
            <path d="M39 101h47v2h7v4H35v-3h4Z" />
          </g>

          <g className="rain-umbrella-sprite-lab__boat">
            <path className="rain-umbrella-sprite-lab__boat-outline" d="M18 99h8l6-7 7 7h7l-5 9H24l-6-9Z" />
            <path className="rain-umbrella-sprite-lab__boat-paper" d="M22 101h20l-3 4H26Z" />
            <path className="rain-umbrella-sprite-lab__boat-fold" d="M27 98l5-5v5ZM33 93l5 5h-5Z" />
            <path className="rain-umbrella-sprite-lab__boat-shadow" d="M26 105h13v2H26Z" />
          </g>

          <g className="rain-umbrella-sprite-lab__cat">
            <g className="rain-umbrella-sprite-lab__cat-body">
              <path className="rain-umbrella-sprite-lab__cat-outline" d="M55 73h18v2h8v4h5v6h3v11h-2v6h-5v4h-7v3H53v-2h-7v-4h-3v-6h-2V87h3v-6h5v-5h6Z" />
              <path className="rain-umbrella-sprite-lab__cat-fur" d="M58 76h14v2h7v4h4v6h2v8h-2v4h-5v3h-6v2H54v-2h-6v-4h-3V88h2v-5h5v-4h6Z" />
              <path className="rain-umbrella-sprite-lab__cat-white" d="M65 82h8v2h5v4h3v8h-2v4h-5v3H62v-2h5v-3h2V88h-4Z" />
              <path className="rain-umbrella-sprite-lab__cat-breath" d="M60 78h11v2h6v4h3v4h-5v-3h-6v-2h-9Z" />
            </g>

            <g className="rain-umbrella-sprite-lab__cat-head">
              <path className="rain-umbrella-sprite-lab__cat-outline" d="M40 85h3v-8h5l4 5h7l5-5h5v9h3v11h-3v4h-5v3H47v-2h-5v-4h-3v-9h1Z" />
              <path className="rain-umbrella-sprite-lab__cat-fur" d="M44 85v-5h2l4 5h11l5-5v8h2v8h-2v3h-4v2H49v-2h-4v-3h-3v-6h2v-5Z" />
              <path className="rain-umbrella-sprite-lab__inner-ear" d="M46 80h2l3 4h-5ZM64 84l2-4v5h-2Z" />
              <path className="rain-umbrella-sprite-lab__cat-white" d="M44 88h7v2h7v-2h9v7h-2v3h-5v3H49v-2h-4v-3h-3v-5h2Z" />
              <path className="rain-umbrella-sprite-lab__eye rain-umbrella-sprite-lab__eye--sleep" d="M47 92h5v1h-5ZM59 92h5v1h-5Z" />
              <path className="rain-umbrella-sprite-lab__eye rain-umbrella-sprite-lab__eye--awake" d="M48 90h3v4h-3ZM60 90h3v4h-3Z" />
              <path className="rain-umbrella-sprite-lab__eye-glint" d="M48 90h1v1h-1ZM60 90h1v1h-1Z" />
              <path className="rain-umbrella-sprite-lab__nose" d="M54 95h3v2h-3Z" />
            </g>

            <path className="rain-umbrella-sprite-lab__paw" d="M43 100h9v4h-3v2h-8v-3h2Z" />
            <path className="rain-umbrella-sprite-lab__tail-base" d="M79 82h6v3h4v11h-3v6h-7v4H63v-5h14v-3h5V88h-3Z" />
            <path className="rain-umbrella-sprite-lab__tail-tip rain-umbrella-sprite-lab__tail-tip--rest" d="M53 101h13v5h-9v-2h-4Z" />
            <path className="rain-umbrella-sprite-lab__tail-tip rain-umbrella-sprite-lab__tail-tip--flick" d="M53 99h5v2h8v5h-9v-2h-4Z" />
          </g>

          <path className="rain-umbrella-sprite-lab__splash rain-umbrella-sprite-lab__splash--a" d="M34 99h2v5h-2ZM30 103h4v2h-4ZM36 103h4v2h-4Z" />
          <path className="rain-umbrella-sprite-lab__splash rain-umbrella-sprite-lab__splash--b" d="M34 98h2v3h-2ZM32 101h2v1h-2ZM36 101h2v1h-2Z" />
          <path className="rain-umbrella-sprite-lab__alarm-mark" d="M99 17h3v9h-3Zm0 12h3v3h-3Z" />
        </g>
      ) : (
        <g className="rain-umbrella-sprite-lab__scene rain-umbrella-sprite-lab__scene--mini">
          <defs>
            <pattern id={rainAId} width="52" height="24" patternUnits="userSpaceOnUse">
              <path className="rain-umbrella-sprite-lab__rain-drop" d="M3 2h1v4H3ZM12 17h1v3h-1ZM45 8h1v4h-1Z" />
            </pattern>
            <pattern id={rainBId} width="52" height="24" patternUnits="userSpaceOnUse">
              <path className="rain-umbrella-sprite-lab__rain-drop" d="M16 6h1v4h-1ZM33 19h1v3h-1ZM49 2h1v4h-1Z" />
            </pattern>
          </defs>

          <rect className="rain-umbrella-sprite-lab__rain-field rain-umbrella-sprite-lab__rain-field--a" x="1" y="-24" width="50" height="100" fill={`url(#${rainAId})`} />
          <rect className="rain-umbrella-sprite-lab__rain-field rain-umbrella-sprite-lab__rain-field--b" x="1" y="-24" width="50" height="100" fill={`url(#${rainBId})`} />

          <g className="rain-umbrella-sprite-lab__puddle">
            <path className="rain-umbrella-sprite-lab__puddle-outline" d="M6 44h9v-1h20v1h7v2h4v3h-4v2H9v-1H5v-4h1Z" />
            <path className="rain-umbrella-sprite-lab__puddle-mid" d="M8 46h8v-1h18v1h7v1h3v1h-5v1H10v-1H7v-1h1Z" />
            <path className="rain-umbrella-sprite-lab__puddle-light" d="M12 46h20v1h6v1H11v-1H8v-1Z" />
          </g>

          <g className="rain-umbrella-sprite-lab__umbrella-rig">
            <g className="rain-umbrella-sprite-lab__umbrella">
              <path className="rain-umbrella-sprite-lab__umbrella-outline" d="M4 14h2v-3h3V8h4V6h5V4h16v2h5v2h4v3h3v4h2v7h-8v2h-5v-2h-5v2h-6v-2h-5v2h-6v-2H4Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-fabric" d="M7 14h1v-3h3V9h4V7h5V6h12v1h5v2h4v2h3v4h1v5h-5v2h-5v-2h-5v2h-6v-2h-5v2h-5v-2H7Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-panel rain-umbrella-sprite-lab__umbrella-panel--light" d="M20 6h5v14h-1v-2h-5v2h-5v-2H9v-4h2v-3h3V9h3V7h3Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-panel rain-umbrella-sprite-lab__umbrella-panel--dark" d="M26 6h6v1h5v2h4v2h3v4h1v5h-5v2h-5v-2h-5v2h-4Z" />
              <path className="rain-umbrella-sprite-lab__umbrella-ribs" d="M25 5h2v15h-2ZM17 8h1v2h1v2h1v6h-1v-5h-1v-2h-1ZM34 8h1v2h1v2h1v6h-1v-5h-1v-2h-1Z" />
            </g>
            <g className="rain-umbrella-sprite-lab__handle">
              <path className="rain-umbrella-sprite-lab__handle-outline" d="M34 20h3v21h2v5h-2v3h-5v-2h-2v-4h3v3h2v-2h1v-2h-2Z" />
              <path className="rain-umbrella-sprite-lab__handle-metal" d="M35 21h1v20h-1ZM36 42h2v3h-1v2h-3v-1h2Z" />
            </g>
          </g>

          <path className="rain-umbrella-sprite-lab__drip rain-umbrella-sprite-lab__drip--left" d="M4 23h1v3H4Z" />
          <path className="rain-umbrella-sprite-lab__drip rain-umbrella-sprite-lab__drip--right" d="M47 23h1v3h-1Z" />

          <g className="rain-umbrella-sprite-lab__boat">
            <path className="rain-umbrella-sprite-lab__boat-outline" d="M3 42h4l4-4 4 4h4l-3 6H6l-3-6Z" />
            <path className="rain-umbrella-sprite-lab__boat-paper" d="M6 43h10l-2 3H7Z" />
            <path className="rain-umbrella-sprite-lab__boat-fold" d="M8 41l3-3v3ZM12 38l3 3h-3Z" />
          </g>

          <g className="rain-umbrella-sprite-lab__cat">
            <g className="rain-umbrella-sprite-lab__cat-body">
              <path className="rain-umbrella-sprite-lab__cat-outline" d="M27 31h7v1h4v2h3v4h2v5h-2v3h-4v2H24v-1h-4v-3h-1v-7h1v-3h3v-2h4Z" />
              <path className="rain-umbrella-sprite-lab__cat-fur" d="M28 33h6v1h3v2h2v3h1v4h-2v2h-3v1H25v-1h-3v-2h-1v-5h1v-3h3v-1h3Z" />
              <path className="rain-umbrella-sprite-lab__cat-white" d="M33 35h3v1h2v2h1v5h-2v2h-6v-1h2v-2h1v-5h-1Z" />
            </g>

            <g className="rain-umbrella-sprite-lab__cat-head">
              <path className="rain-umbrella-sprite-lab__cat-outline" d="M16 35h1v-5h3l2 3h3l3-3h3v5h2v6h-1v3h-3v2H19v-1h-3v-2h-2v-5h1v-3h1Z" />
              <path className="rain-umbrella-sprite-lab__cat-fur" d="M18 35v-3l2 3h6l3-3v4h1v5h-1v2h-2v1h-7v-1h-2v-2h-2v-3h1v-3h1Z" />
              <path className="rain-umbrella-sprite-lab__inner-ear" d="M19 32l2 2h-2ZM28 32v2h-2Z" />
              <path className="rain-umbrella-sprite-lab__cat-white" d="M18 37h3v1h3v-1h5v4h-1v2h-3v1h-5v-1h-2v-2h-2v-3h2Z" />
              <path className="rain-umbrella-sprite-lab__eye rain-umbrella-sprite-lab__eye--sleep" d="M19 39h2v1h-2ZM25 39h2v1h-2Z" />
              <path className="rain-umbrella-sprite-lab__eye rain-umbrella-sprite-lab__eye--awake" d="M19 38h2v2h-2ZM25 38h2v2h-2Z" />
              <path className="rain-umbrella-sprite-lab__nose" d="M22 41h2v1h-2Z" />
            </g>

            <path className="rain-umbrella-sprite-lab__tail-base" d="M36 35h4v2h2v5h-2v3h-3v2h-7v-2h6v-2h3v-5h-3Z" />
            <path className="rain-umbrella-sprite-lab__tail-tip rain-umbrella-sprite-lab__tail-tip--rest" d="M24 45h8v2h-6v-1h-2Z" />
            <path className="rain-umbrella-sprite-lab__tail-tip rain-umbrella-sprite-lab__tail-tip--flick" d="M24 44h3v1h5v2h-6v-1h-2Z" />
          </g>

          <path className="rain-umbrella-sprite-lab__splash rain-umbrella-sprite-lab__splash--a" d="M13 43h1v3h-1ZM11 45h2v1h-2ZM14 45h2v1h-2Z" />
          <path className="rain-umbrella-sprite-lab__splash rain-umbrella-sprite-lab__splash--b" d="M13 42h1v2h-1ZM12 44h1v1h-1ZM14 44h1v1h-1Z" />
          <path className="rain-umbrella-sprite-lab__alarm-mark" d="M44 5h2v5h-2Zm0 7h2v2h-2Z" />
        </g>
      )}
    </svg>
  );
}
