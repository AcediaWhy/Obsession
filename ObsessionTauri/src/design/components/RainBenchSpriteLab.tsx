import { useId } from "react";
import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./RainBenchSpriteLab.css";

export type RainBenchMood = "idle" | "busy" | "scanning" | "active" | "alarm";
export type RainBenchDetail = "auto" | "base" | "hero";

export function rainBenchMoodForPhase(phase: ObsessionVisualPhase): RainBenchMood {
  if (phase === "fault") return "alarm";
  if (phase === "focused") return "active";
  if (phase === "engaging") return "busy";
  return phase;
}

type RainBenchSpriteLabProps = {
  phase: ObsessionVisualPhase;
  detail?: RainBenchDetail;
  paused?: boolean;
  size?: number;
};

/**
 * Две независимые целочисленные пиксельные сетки:
 * 120×120 для hero (ровно ×2 в 240 px) и 52×52 для preview (ровно ×2 в 104 px).
 * Силуэт кота не трансформируется: движение живёт только в свете, дожде и луже.
 */
export function RainBenchSpriteLab({
  phase,
  detail = "auto",
  paused = false,
  size = 240,
}: RainBenchSpriteLabProps) {
  const mood = rainBenchMoodForPhase(phase);
  const resolvedDetail = size >= 180 && detail !== "base" ? "hero" : "base";
  const hero = resolvedDetail === "hero";
  const rainPatternKey = useId().replace(/:/g, "");
  const rainAId = `${rainPatternKey}-rain-a`;
  const rainBId = `${rainPatternKey}-rain-b`;
  const rainFrontId = `${rainPatternKey}-rain-front`;

  return (
    <svg
      aria-hidden="true"
      className="rain-bench-sprite-lab"
      data-detail={resolvedDetail}
      data-mood={mood}
      data-motion={paused ? "still" : "running"}
      data-rain-bench-sprite
      focusable="false"
      shapeRendering="crispEdges"
      style={{ width: size, height: size }}
      viewBox={hero ? "0 0 120 120" : "0 0 52 52"}
    >
      {hero ? (
        <g className="rain-bench-sprite-lab__scene rain-bench-sprite-lab__scene--hero">
          <defs>
            <pattern id={rainAId} width="120" height="48" patternUnits="userSpaceOnUse">
              <path className="rain-bench-sprite-lab__rain-drop" d="M12 4h1v7h-1ZM42 31h1v6h-1ZM78 15h1v9h-1ZM108 39h1v6h-1Z" />
            </pattern>
            <pattern id={rainBId} width="120" height="48" patternUnits="userSpaceOnUse">
              <path className="rain-bench-sprite-lab__rain-drop" d="M28 10h1v6h-1ZM62 38h1v7h-1ZM96 24h1v8h-1Z" />
            </pattern>
            <pattern id={rainFrontId} width="120" height="48" patternUnits="userSpaceOnUse">
              <path className="rain-bench-sprite-lab__rain-drop-front" d="M20 21h1v9h-1ZM87 34h1v10h-1Z" />
            </pattern>
          </defs>

          <g className="rain-bench-sprite-lab__stars">
            <path d="M18 14h2v2h-2ZM39 7h2v2h-2ZM74 14h2v2h-2ZM91 9h2v2h-2Z" />
          </g>

          <path className="rain-bench-sprite-lab__shore-far" d="M6 58h11v-5h15v3h16v-3h20v4h16v-5h14v3h15v12h-8v3H90v-2H75v4H57v-3H39v3H24v-2H11v-4H6Z" />
          <path className="rain-bench-sprite-lab__shore-near" d="M10 68h10v-5h18v4h20v-3h18v4h19v-4h15v19h-7v4H88v-2H70v4H50v-3H32v2H18v-4H10Z" />
          <path className="rain-bench-sprite-lab__ground" d="M7 79h16v-3h19v3h23v-2h21v2h24v14h5v9h-6v6H96v5H82v4H42v-3H25v-5H13v-7H7Z" />
          <path className="rain-bench-sprite-lab__light-pool" d="M55 63h30v3h14v5h10v25h-7v8H84v4H45v-3H24v-9H34V84h9V72h12Z" />

          <rect className="rain-bench-sprite-lab__rain-field rain-bench-sprite-lab__rain-field--a" x="0" y="-48" width="120" height="216" fill={`url(#${rainAId})`} />
          <rect className="rain-bench-sprite-lab__rain-field rain-bench-sprite-lab__rain-field--b" x="0" y="-48" width="120" height="216" fill={`url(#${rainBId})`} />

          <g className="rain-bench-sprite-lab__lamp">
            <path className="rain-bench-sprite-lab__lamp-outline" d="M101 4h9v79h5v8h-3v3H98v-3h-3v-8h5V4Z" />
            <path className="rain-bench-sprite-lab__lamp-mid" d="M102 5h4v77h-4ZM100 84h10v6h-10Z" />
            <path className="rain-bench-sprite-lab__lamp-edge" d="M106 5h2v77h-2ZM102 85h7v2h-7Z" />
            <path className="rain-bench-sprite-lab__lamp-glint" d="M104 8h1v24h-1ZM107 44h1v16h-1Z" />
          </g>

          <g className="rain-bench-sprite-lab__puddle">
            <path className="rain-bench-sprite-lab__puddle-dark" d="M12 90h28v2h11v4h6v7h-5v5H42v3H17v-2H8v-4H5v-8h4v-5h3Z" />
            <path className="rain-bench-sprite-lab__puddle-mid" d="M14 94h25v2h11v3h4v4h-6v3H18v-2H10v-5h4Z" />
            <path className="rain-bench-sprite-lab__puddle-light" d="M17 96h20v2h9v2h-7v2H16v-1h-5v-3h6Z" />
            <path className="rain-bench-sprite-lab__puddle-warm" d="M31 101h11v2H31Z" />
            <path className="rain-bench-sprite-lab__ripple rain-bench-sprite-lab__ripple--one" d="M42 105h10v1H42Z" />
            <path className="rain-bench-sprite-lab__ripple rain-bench-sprite-lab__ripple--two" d="M38 108h18v1H38Z" />
            <path className="rain-bench-sprite-lab__ripple rain-bench-sprite-lab__ripple--three" d="M33 111h28v1H33Z" />
            <path className="rain-bench-sprite-lab__splash rain-bench-sprite-lab__splash--a" d="M20 89h1v4h-1ZM17 92h3v1h-3ZM21 92h3v1h-3Z" />
            <path className="rain-bench-sprite-lab__splash rain-bench-sprite-lab__splash--b" d="M20 87h1v3h-1ZM18 90h2v1h-2ZM21 90h2v1h-2Z" />
          </g>

          <path className="rain-bench-sprite-lab__bench-shadow" d="M20 91h81v2h9v6h-12v4H30v-2H15v-7h5Z" />
          <g className="rain-bench-sprite-lab__bench">
            <path className="rain-bench-sprite-lab__bench-outline" d="M24 44h6v39h-6ZM91 44h7v40h-7ZM20 46h78v12H20ZM19 59h79v12H19ZM18 72h80v11H18ZM15 80h85v16H15ZM26 94h7v18h-7ZM89 93h7v20h-7Z" />
            <path className="rain-bench-sprite-lab__bench-shadow-wood" d="M22 51h74v5H22ZM21 64h75v5H21ZM20 77h76v4H20ZM16 92h83v2H16Z" />
            <path className="rain-bench-sprite-lab__bench-wood" d="M22 48h74v6H22ZM21 61h75v6H21ZM20 74h76v5H20ZM17 81h81v3H17ZM16 85h83v3H16ZM16 89h83v3H16Z" />
            <path className="rain-bench-sprite-lab__bench-frame" d="M25 45h4v37h-4ZM92 45h5v38h-5Z" />
            <path className="rain-bench-sprite-lab__bench-light" d="M23 48h44v1H23ZM71 48h24v1H71ZM22 61h34v1H22ZM60 61h35v1H60ZM21 74h43v1H21ZM68 74h27v1H68ZM19 81h40v1H19ZM18 85h32v1H18ZM18 89h44v1H18Z" />
            <path className="rain-bench-sprite-lab__wood-grain" d="M34 52h12v1H34ZM73 52h9v1H73ZM39 65h14v1H39ZM77 65h10v1H77ZM32 77h8v1H32ZM52 77h17v1H52ZM27 83h15v1H27ZM68 87h13v1H68ZM30 91h19v1H30Z" />
            <path className="rain-bench-sprite-lab__bench-endcap" d="M20 48h2v8h-2ZM94 48h2v8h-2ZM19 61h2v8h-2ZM94 61h2v8h-2ZM18 74h2v7h-2ZM94 74h2v7h-2Z" />
            <path className="rain-bench-sprite-lab__bench-nail" d="M27 51h2v2h-2ZM93 51h2v2h-2ZM27 64h2v2h-2ZM93 64h2v2h-2ZM27 76h2v2h-2ZM93 76h2v2h-2Z" />
            <path className="rain-bench-sprite-lab__bench-metal" d="M28 94h5v18h-5ZM90 94h5v19h-5ZM33 103h57v4H33ZM25 110h11v3H25ZM88 111h10v3H88Z" />
            <path className="rain-bench-sprite-lab__metal-glint" d="M29 96h1v12h-1ZM91 96h1v13h-1ZM35 104h36v1H35Z" />
          </g>

          <g className="rain-bench-sprite-lab__cat-shadow">
            <path d="M50 91h40v1h6v3H48v-3h2Z" />
          </g>
          <g className="rain-bench-sprite-lab__cat" transform="translate(0 -4)">
            <g className="rain-bench-sprite-lab__cat-body">
              <path className="rain-bench-sprite-lab__cat-outline" d="M69 70h13v2h6v3h4v4h3v9h-2v4h-5v3H68v-2h-6v-4h-3V78h3v-5h7Z" />
              <path className="rain-bench-sprite-lab__cat-fur" d="M70 73h11v2h6v3h4v10h-4v3H69v-2h-5v-4h-2v-6h3v-4h5Z" />
              <path className="rain-bench-sprite-lab__cat-white" d="M76 75h6v2h5v3h3v7h-3v3H77v-2h-4v-4h-2v-5h3v-3h2Z" />
              <path className="rain-bench-sprite-lab__cat-white-shadow" d="M82 87h6v2h-3v2h-8v-2h5Z" />
              <path className="rain-bench-sprite-lab__cat-breath" d="M79 76h4v2h4v3h2v3h-2v-2h-4v-3h-4Z" />
            </g>

            <g className="rain-bench-sprite-lab__cat-head">
              <path className="rain-bench-sprite-lab__cat-outline" d="M55 70h2v-7h4l3 4h4l3-4h4v8h3v11h-2v4h-4v3H58v-2h-5v-3h-3v-8h2v-4h3Z" />
              <path className="rain-bench-sprite-lab__cat-fur" d="M58 70v-4h2l3 4h6l3-4h1v7h2v8h-2v3h-4v2H59v-2h-4v-3h-2v-4h2v-5h3Z" />
              <path className="rain-bench-sprite-lab__inner-ear" d="M59 66h2l2 3h-4ZM71 69l2-3v4h-2Z" />
              <path className="rain-bench-sprite-lab__cat-white" d="M59 73h5v2h5v-2h4v8h-2v3h-4v2h-8v-2h-3v-3h-2v-4h3v-3h2Z" />
              <path className="rain-bench-sprite-lab__cat-white-shadow" d="M56 81h4v2h8v2H59v-1h-3Z" />
              <path className="rain-bench-sprite-lab__eye rain-bench-sprite-lab__eye--sleep" d="M58 77h5v1h-5ZM68 77h4v1h-4Z" />
              <path className="rain-bench-sprite-lab__eye rain-bench-sprite-lab__eye--awake" d="M59 76h2v3h-2ZM69 76h2v3h-2Z" />
              <path className="rain-bench-sprite-lab__nose" d="M53 79h3v2h-3Z" />
            </g>

            <path className="rain-bench-sprite-lab__paw" d="M58 86h9v3h-9ZM66 88h10v3H66Z" />
            <path className="rain-bench-sprite-lab__tail rain-bench-sprite-lab__tail--loose" d="M88 80h5v3h3v8h-3v4h-6v3H72v-4h15v-2h4v-3h1v-4h-4Z" />
            <path className="rain-bench-sprite-lab__tail rain-bench-sprite-lab__tail--tucked" d="M87 79h6v4h3v8h-3v4h-7v2H68v-4h19v-2h4v-7h-4Z" />
          </g>

          <rect className="rain-bench-sprite-lab__rain-front" x="0" y="-48" width="120" height="216" fill={`url(#${rainFrontId})`} />
          <path className="rain-bench-sprite-lab__lightning" d="M49 55h39v4h15v9h8v28h-9v10H84v7H42v-6H25V96H15V77h10V66h12v-7h12Z" />
        </g>
      ) : (
        <g className="rain-bench-sprite-lab__scene rain-bench-sprite-lab__scene--mini">
          <defs>
            <pattern id={rainAId} width="52" height="24" patternUnits="userSpaceOnUse">
              <path className="rain-bench-sprite-lab__rain-drop" d="M8 3h1v4H8ZM34 15h1v5h-1Z" />
            </pattern>
            <pattern id={rainBId} width="52" height="24" patternUnits="userSpaceOnUse">
              <path className="rain-bench-sprite-lab__rain-drop" d="M22 8h1v4h-1ZM48 19h1v3h-1Z" />
            </pattern>
            <pattern id={rainFrontId} width="52" height="24" patternUnits="userSpaceOnUse">
              <path className="rain-bench-sprite-lab__rain-drop-front" d="M14 12h1v5h-1Z" />
            </pattern>
          </defs>

          <path className="rain-bench-sprite-lab__shore-far" d="M3 25h7v-2h8v2h8v-1h10v2h7v-2h6v7h-4v2h-8v-1h-8v2H18v-2H9v1H4v-3H3Z" />
          <path className="rain-bench-sprite-lab__ground" d="M3 34h9v-2h11v2h10v-1h16v7h2v5h-3v3H39v2H18v-2H9v-3H4v-5H3Z" />
          <path className="rain-bench-sprite-lab__light-pool" d="M24 28h12v1h7v3h5v11h-4v3H29v1H13v-4h4v-5h3v-6h4Z" />

          <rect className="rain-bench-sprite-lab__rain-field rain-bench-sprite-lab__rain-field--a" x="0" y="-24" width="52" height="100" fill={`url(#${rainAId})`} />
          <rect className="rain-bench-sprite-lab__rain-field rain-bench-sprite-lab__rain-field--b" x="0" y="-24" width="52" height="100" fill={`url(#${rainBId})`} />

          <g className="rain-bench-sprite-lab__lamp">
            <path className="rain-bench-sprite-lab__lamp-outline" d="M43 2h5v35h3v5h-1v1h-9v-1h-1v-5h2V2Z" />
            <path className="rain-bench-sprite-lab__lamp-mid" d="M43 3h2v34h-2ZM42 38h6v3h-6Z" />
            <path className="rain-bench-sprite-lab__lamp-edge" d="M45 3h1v34h-1Z" />
            <path className="rain-bench-sprite-lab__lamp-glint" d="M44 4h1v12h-1Z" />
          </g>

          <g className="rain-bench-sprite-lab__puddle">
            <path className="rain-bench-sprite-lab__puddle-dark" d="M6 40h12v1h5v2h3v4h-3v2H8v-1H4v-5h2Z" />
            <path className="rain-bench-sprite-lab__puddle-mid" d="M7 42h11v1h5v2h-3v2H8v-1H5v-3h2Z" />
            <path className="rain-bench-sprite-lab__puddle-light" d="M9 42h8v1h4v1h-5v1H7v-1H6v-1h3Z" />
            <path className="rain-bench-sprite-lab__ripple rain-bench-sprite-lab__ripple--one" d="M17 46h5v1h-5Z" />
            <path className="rain-bench-sprite-lab__ripple rain-bench-sprite-lab__ripple--two" d="M15 48h9v1h-9Z" />
            <path className="rain-bench-sprite-lab__splash rain-bench-sprite-lab__splash--a" d="M8 39h1v3H8ZM6 41h2v1H6ZM9 41h2v1H9Z" />
            <path className="rain-bench-sprite-lab__splash rain-bench-sprite-lab__splash--b" d="M8 38h1v2H8ZM7 40h1v1H7ZM9 40h1v1H9Z" />
          </g>

          <g className="rain-bench-sprite-lab__bench">
            <path className="rain-bench-sprite-lab__bench-outline" d="M10 18h3v18h-3ZM39 18h3v18h-3ZM8 19h36v6H8ZM7 25h37v6H7ZM7 31h37v5H7ZM6 35h41v7H6ZM11 41h3v9h-3ZM38 40h3v10h-3Z" />
            <path className="rain-bench-sprite-lab__bench-shadow-wood" d="M9 22h34v2H9ZM8 28h35v2H8ZM8 34h35v1H8ZM7 41h39v1H7Z" />
            <path className="rain-bench-sprite-lab__bench-wood" d="M9 20h34v3H9ZM8 26h35v3H8ZM8 32h35v3H8ZM7 36h39v1H7ZM7 38h39v1H7ZM7 40h39v1H7Z" />
            <path className="rain-bench-sprite-lab__bench-frame" d="M10 19h2v17h-2ZM40 19h2v17h-2Z" />
            <path className="rain-bench-sprite-lab__bench-light" d="M10 20h18v1H10ZM30 20h12v1H30ZM9 26h15v1H9ZM27 26h15v1H27ZM9 32h19v1H9ZM8 36h18v1H8ZM8 38h14v1H8Z" />
            <path className="rain-bench-sprite-lab__wood-grain" d="M15 22h6v1h-6ZM27 28h7v1h-7ZM14 34h8v1h-8ZM9 40h9v1H9Z" />
            <path className="rain-bench-sprite-lab__bench-metal" d="M12 41h2v9h-2ZM38 41h2v9h-2ZM14 46h24v2H14Z" />
          </g>

          <g className="rain-bench-sprite-lab__cat" transform="translate(0 -2)">
            <path className="rain-bench-sprite-lab__cat-outline" d="M24 31h1v-4h2l2 2h2l2-2h2v4h2v2h4v2h2v5h-1v2h-3v2H26v-1h-3v-2h-2v-6h1v-3h2Z" />
            <path className="rain-bench-sprite-lab__cat-fur" d="M25 32v-3h1l2 2h4l2-2v4h3v1h3v2h1v4h-3v2H27v-1h-3v-2h-1v-3h1v-3h1Z" />
            <path className="rain-bench-sprite-lab__cat-white" d="M25 33h3v1h3v-1h3v4h2v3h-2v2h-7v-1h-2v-2h-1v-4h1Z" />
            <path className="rain-bench-sprite-lab__cat-breath" d="M34 34h3v1h2v2h-1v1h-2v-2h-2Z" />
            <path className="rain-bench-sprite-lab__eye rain-bench-sprite-lab__eye--sleep" d="M25 35h3v1h-3ZM31 35h2v1h-2Z" />
            <path className="rain-bench-sprite-lab__eye rain-bench-sprite-lab__eye--awake" d="M26 34h1v2h-1ZM32 34h1v2h-1Z" />
            <path className="rain-bench-sprite-lab__nose" d="M22 36h2v1h-2Z" />
            <path className="rain-bench-sprite-lab__tail rain-bench-sprite-lab__tail--loose" d="M39 36h3v2h2v4h-2v2h-4v1h-9v-2h9v-1h3v-4h-2Z" />
            <path className="rain-bench-sprite-lab__tail rain-bench-sprite-lab__tail--tucked" d="M39 35h3v2h2v5h-2v2h-5v1H28v-2h10v-1h3v-5h-2Z" />
          </g>

          <rect className="rain-bench-sprite-lab__rain-front" x="0" y="-24" width="52" height="100" fill={`url(#${rainFrontId})`} />

          <path className="rain-bench-sprite-lab__lightning" d="M21 22h17v2h7v4h4v13h-4v5h-8v4H18v-3H9v-5H4v-9h5v-5h5v-3h7Z" />
        </g>
      )}
    </svg>
  );
}
