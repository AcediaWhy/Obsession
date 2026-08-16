import type { CSSProperties } from "react";

import {
  obsessionFocusForScreen,
  type ObsessionVisualPhase,
} from "../obsessionVisualState";

export type ObsessionSceneProps = {
  paused?: boolean;
  phase: ObsessionVisualPhase;
  screen: string;
};

const THREADS = [
  [0, 8], [0, 31], [0, 68], [0, 91],
  [100, 12], [100, 42], [100, 76], [100, 94],
  [14, 0], [42, 0], [68, 0], [94, 0],
  [8, 100], [38, 100], [66, 100], [91, 100],
] as const;

/** Composition-matched 2D scene used during shader compilation and WebGL failure. */
export function ObsessionFallback({
  paused = false,
  phase,
  screen,
}: ObsessionSceneProps) {
  const focus = obsessionFocusForScreen(screen);
  const x = focus.x * 100;
  const y = focus.y * 100;
  const style = {
    "--obsession-focus-x": `${x}%`,
    "--obsession-focus-y": `${y}%`,
  } as CSSProperties;

  return (
    <div
      data-testid="obsession-fallback"
      data-obsession-phase={phase}
      data-paused={paused || undefined}
      className="obsession-fallback pointer-events-none absolute inset-0 overflow-hidden bg-[#030305]"
      style={style}
    >
      <div className="obsession-fallback-depth absolute inset-0" />
      <svg
        aria-hidden="true"
        className="absolute inset-0 h-full w-full"
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
      >
        <defs>
          <radialGradient id="obsession-fallback-lens" cx="42%" cy="38%">
            <stop offset="0" stopColor="#bfc0bd" stopOpacity=".72" />
            <stop offset=".15" stopColor="#4d0715" stopOpacity=".82" />
            <stop offset=".46" stopColor="#17030a" stopOpacity=".82" />
            <stop offset=".78" stopColor="#c8c2bc" stopOpacity=".14" />
            <stop offset="1" stopColor="#050507" stopOpacity="0" />
          </radialGradient>
          <linearGradient id="obsession-fallback-thread">
            <stop offset="0" stopColor="#c7c2bc" stopOpacity="0" />
            <stop offset=".72" stopColor="#d8d2cc" stopOpacity=".16" />
            <stop offset="1" stopColor="#5e0c20" stopOpacity=".32" />
          </linearGradient>
          <radialGradient id="obsession-fallback-sclera" cx="38%" cy="30%">
            <stop offset="0" stopColor="#eee9e4" stopOpacity=".82" />
            <stop offset=".35" stopColor="#777579" stopOpacity=".52" />
            <stop offset=".72" stopColor="#1d1c20" stopOpacity=".9" />
            <stop offset="1" stopColor="#040406" />
          </radialGradient>
          <radialGradient id="obsession-fallback-iris" cx="38%" cy="34%">
            <stop offset="0" stopColor="#d3bfc0" stopOpacity=".85" />
            <stop offset=".18" stopColor="#79152f" stopOpacity=".96" />
            <stop offset=".68" stopColor="#27040f" />
            <stop offset="1" stopColor="#030204" />
          </radialGradient>
          <clipPath id="obsession-fallback-eye-clip">
            <path d="M-9.8 0 C-5.5 -5.2 5.5 -5.2 9.8 0 C5.5 5.2 -5.5 5.2 -9.8 0Z" />
          </clipPath>
        </defs>
        <g className="obsession-fallback-threads">
          {THREADS.map(([tx, ty], index) => (
            <line
              key={`${tx}-${ty}`}
              x1={tx}
              y1={ty}
              x2={x}
              y2={y}
              vectorEffect="non-scaling-stroke"
              stroke="url(#obsession-fallback-thread)"
              strokeWidth={index % 3 === 0 ? 0.55 : 0.3}
            />
          ))}
        </g>
        <g className="obsession-fallback-core" transform={`translate(${x} ${y})`}>
          <ellipse rx="22.5" ry="22.5" fill="none" stroke="#d9d3cc" strokeOpacity=".08" strokeWidth=".2" strokeDasharray="1.2 4" vectorEffect="non-scaling-stroke" />
          <ellipse rx="19.2" ry="19.2" fill="none" stroke="#b82444" strokeOpacity=".16" strokeWidth=".28" strokeDasharray="6 3" vectorEffect="non-scaling-stroke" />
          <ellipse rx="14.8" ry="14.8" fill="url(#obsession-fallback-lens)" opacity=".38" />
          <ellipse rx="13.6" ry="12.5" fill="none" stroke="#d9d3cc" strokeOpacity=".15" strokeWidth=".24" strokeDasharray="1 3.2" vectorEffect="non-scaling-stroke" />
          <ellipse rx="11.3" ry="10.4" fill="none" stroke="#78142f" strokeOpacity=".25" strokeWidth=".34" strokeDasharray="4 2" vectorEffect="non-scaling-stroke" />
          <line x1="-22" y1="0" x2="22" y2="0" stroke="#d9d3cc" strokeOpacity=".08" strokeWidth=".2" vectorEffect="non-scaling-stroke" />
          <line x1="0" y1="-22" x2="0" y2="22" stroke="#d9d3cc" strokeOpacity=".08" strokeWidth=".2" vectorEffect="non-scaling-stroke" />
          <g className="obsession-fallback-eye">
            <g clipPath="url(#obsession-fallback-eye-clip)">
              <rect x="-10.5" y="-6.5" width="21" height="13" fill="url(#obsession-fallback-sclera)" />
              <ellipse className="obsession-fallback-iris" rx="3.2" ry="3.2" fill="url(#obsession-fallback-iris)" />
              <ellipse className="obsession-fallback-aperture" rx="1.0" ry="1.0" fill="#020203" stroke="#ece5df" strokeOpacity=".52" strokeWidth=".22" vectorEffect="non-scaling-stroke" />
              <circle cx="-0.9" cy="-1.1" r="0.85" fill="#faf5ef" opacity=".7" />
              <path d="M-8 -2.8 Q-1.5 -5.2 6.5 -3.0" fill="none" stroke="#fffaf4" strokeOpacity=".18" strokeWidth=".5" vectorEffect="non-scaling-stroke" />
            </g>
            <path className="obsession-fallback-lids" d="M-9.8 0 C-5.5 -5.2 5.5 -5.2 9.8 0 C5.5 5.2 -5.5 5.2 -9.8 0Z" fill="none" stroke="#e8e1db" strokeOpacity=".6" strokeWidth=".55" vectorEffect="non-scaling-stroke" />
          </g>
        </g>
        <path d="M-1 -1" />
      </svg>
      <div className="obsession-fallback-vignette absolute inset-0" />
    </div>
  );
}
