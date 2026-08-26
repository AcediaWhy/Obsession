import type { ObsessionVisualPhase } from "../obsessionVisualState";
import "./YaniCatSpriteLab.css";

type CatMood = "idle" | "busy" | "scanning" | "active" | "alarm";

type YaniCatSpriteLabProps = {
  phase: ObsessionVisualPhase;
  paused?: boolean;
  standalone?: boolean;
};

export function YaniCatSpriteLab({ phase, paused = false, standalone = false }: YaniCatSpriteLabProps) {
  const mood: CatMood = phase === "fault"
    ? "alarm"
    : phase === "focused"
      ? "active"
      : phase === "engaging"
        ? "busy"
        : phase;

  return (
    <svg
      aria-hidden="true"
      className="yani-cat-sprite-lab"
      data-mood={mood}
      data-motion={paused ? "still" : "running"}
      data-presentation={standalone ? "standalone" : "device"}
      focusable="false"
      viewBox="0 0 240 240"
    >
      <g
        className="yani-cat-sprite-lab__sprite"
        shapeRendering="crispEdges"
        transform={standalone ? "translate(40 28) scale(8)" : "translate(92 70) scale(3)"}
      >
        <g className="yani-cat-sprite-lab__tail">
          <path className="yani-cat-sprite-lab__outline" d="M13 17h3v1h2v-2h1v-6h2v8h-1v2h-2v2h-5Z" />
          <path className="yani-cat-sprite-lab__fur" d="M15 18h2v1h1v-2h1v-5h1v6h-1v1h-2v1h-2Z" />
          <path className="yani-cat-sprite-lab__tail-tip" d="M19 10h2v4h-2Z" />
        </g>

        <path className="yani-cat-sprite-lab__outline" d="M4 13h11v2h2v6h-3v-2h-2v3H7v-3H5v2H2v-6h2Z" />
        <path className="yani-cat-sprite-lab__fur" d="M5 14h9v2h1v3h-2v-2h-2v4H8v-4H6v2H4v-3h1Z" />
        <path className="yani-cat-sprite-lab__belly" d="M8 15h4v5H8Z" />
        <path className="yani-cat-sprite-lab__paw yani-cat-sprite-lab__paw--left" d="M4 19h4v2H3v-1h1Z" />
        <path className="yani-cat-sprite-lab__paw yani-cat-sprite-lab__paw--right" d="M11 19h4v1h1v1h-5Z" />

        <g className="yani-cat-sprite-lab__head">
          <path className="yani-cat-sprite-lab__outline" d="M2 5V0h3v2h2v2h6V2h2V0h3v5h2v8h-2v2h-3v1H5v-1H2v-2H0V5Z" />
          <path className="yani-cat-sprite-lab__fur" d="M3 6V2h1v2h3v2h6V4h3V2h1v4h1v6h-2v2H4v-2H2V6Z" />
          <path className="yani-cat-sprite-lab__inner-ear yani-cat-sprite-lab__inner-ear--left" d="M3 1h1v2h2v2H3Z" />
          <path className="yani-cat-sprite-lab__inner-ear yani-cat-sprite-lab__inner-ear--right" d="M16 1h1v4h-3V3h2Z" />
          <path className="yani-cat-sprite-lab__hair" d="M5 6h8v1h4v2h-4v1h-3V9H7v1H4V8h1Z" />
          <path className="yani-cat-sprite-lab__muzzle" d="M6 10h3v-1h2v1h3v4h-2v1H8v-1H6Z" />
          <path className="yani-cat-sprite-lab__blush" d="M3 11h2v1H3Zm12 0h2v1h-2Z" />
          <path className="yani-cat-sprite-lab__nose" d="M9 11h2v1H9Zm1 1h1v2h-1Z" />
          <path className="yani-cat-sprite-lab__whiskers" d="M0 10h4v1H0Z M1 13h3v1H1Z M16 10h4v1h-4Z M16 13h3v1h-3Z" />
          <circle className="yani-cat-sprite-lab__earring" cx="2.5" cy="7.5" r="0.7" />

          <g className="yani-cat-sprite-lab__expression yani-cat-sprite-lab__expression--idle">
            <path d="M5 10h3v1H5Zm7 0h3v1h-3Z" />
          </g>
          <g className="yani-cat-sprite-lab__expression yani-cat-sprite-lab__expression--awake">
            <path d="M5 9h3v3H5Zm7 0h3v3h-3Z" />
            <path className="yani-cat-sprite-lab__eye-glint" d="M6 9h1v1H6Zm7 0h1v1h-1Z" />
          </g>
          <g className="yani-cat-sprite-lab__expression yani-cat-sprite-lab__expression--active">
            <path d="M5 10h1V9h2v1H7v1H6v-1Zm7 0h1V9h2v1h-1v1h-1v-1Z" />
          </g>
          <g className="yani-cat-sprite-lab__expression yani-cat-sprite-lab__expression--alarm">
            <path d="M5 8h3v4H5Zm7 0h3v4h-3Z" />
            <path className="yani-cat-sprite-lab__eye-glint" d="M6 8h1v2H6Zm7 0h1v2h-1Z" />
          </g>
        </g>

        <g className="yani-cat-sprite-lab__zzz">
          <path d="M17 4h3v1h-1v1h1v1h-3V6h1V5h-1Zm2-4h2v1h-1v1h1v1h-3V2h1V1h-1Z" />
        </g>
        <path className="yani-cat-sprite-lab__heart" d="M18 5h1V4h2v1h1v2h-1v1h-1v1h-1V8h-1Z" />
        <path className="yani-cat-sprite-lab__alarm-mark" d="M19 2h2v4h-2Zm0 5h2v2h-2Z" />
      </g>
    </svg>
  );
}
