import type { CSSProperties } from "react";

import type { ObsessionVisualPhase } from "../obsessionVisualState";
import { CHOIR_CURVES, CHOIR_EYES, choirCurveToSvgPath } from "./obsessionChoir/geometry";
import { obsessionChoirFocusForScreen } from "./obsessionChoir/layout";
import { sampleObsessionChoirMotion } from "./obsessionChoir/motion";

export type ObsessionChoirSceneProps = {
  phase: ObsessionVisualPhase;
  screen: string;
  paused?: boolean;
  forceRitual?: boolean;
};

export function ObsessionChoirFallback({
  phase,
  screen,
  paused = false,
  forceRitual = false,
}: ObsessionChoirSceneProps) {
  const focus = obsessionChoirFocusForScreen(screen);
  const motion = sampleObsessionChoirMotion({
    time: 8.4,
    phase,
    phaseAge: 1.4,
    forceRitual,
  });
  const focusX = focus.x * 1000;
  const focusY = (1 - focus.y) * 680;
  const masterRx = 154 * (0.965 + motion.masterPulse * 0.07);
  const masterRy = 85 * motion.masterLidOpen * (0.9 + motion.masterPulse * 0.2);
  const masterDx = (motion.masterBodyX - 0.5) * 76;
  const masterDy = (motion.masterBodyY - 0.5) * -52;
  const masterRoll = (motion.masterRoll - 0.5) * 11.5;
  const leftCornerY = focusY + masterRy * 0.09;
  const rightCornerY = focusY - masterRy * 0.09;
  const masterPath = [
    `M ${focusX - masterRx} ${leftCornerY}`,
    `C ${focusX - masterRx * 0.56} ${focusY - masterRy * 0.92}, ${focusX + masterRx * 0.38} ${focusY - masterRy * 1.08}, ${focusX + masterRx} ${rightCornerY}`,
    `C ${focusX + masterRx * 0.48} ${focusY + masterRy * 0.72}, ${focusX - masterRx * 0.42} ${focusY + masterRy * 0.62}, ${focusX - masterRx} ${leftCornerY} Z`,
  ].join(" ");
  const upperFoldPath = [
    `M ${focusX - masterRx * 0.88} ${leftCornerY - masterRy * 0.16}`,
    `C ${focusX - masterRx * 0.42} ${focusY - masterRy * 1.18}, ${focusX + masterRx * 0.44} ${focusY - masterRy * 1.27}, ${focusX + masterRx * 0.88} ${rightCornerY - masterRy * 0.12}`,
  ].join(" ");

  return (
    <div
      aria-hidden="true"
      className="obsession-choir-fallback absolute inset-0 overflow-hidden"
      data-phase={phase}
      data-paused={paused ? "true" : undefined}
      style={{
        "--choir-reveal": motion.chorusReveal.toFixed(3),
        "--choir-carmine": motion.carmineDepth.toFixed(3),
      } as CSSProperties}
    >
      <svg className="absolute inset-0 h-full w-full" viewBox="0 0 1000 680" preserveAspectRatio="xMidYMid slice">
        <defs>
          <radialGradient id="choir-void" cx="44%" cy="38%">
            <stop offset="0" stopColor="#120008" />
            <stop offset="0.32" stopColor="#030104" />
            <stop offset="1" stopColor="#000001" />
          </radialGradient>
          <radialGradient id="choir-iris" cx="42%" cy="37%">
            <stop offset="0" stopColor="#7f1736" />
            <stop offset="0.34" stopColor="#3b0518" />
            <stop offset="0.76" stopColor="#090105" />
            <stop offset="1" stopColor="#000" />
          </radialGradient>
          <filter id="choir-soft-glow" x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="4" result="blur" />
            <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
          </filter>
          <clipPath id="choir-master-clip"><path d={masterPath} /></clipPath>
        </defs>
        <rect width="1000" height="680" fill="#020204" />
        <g className="choir-fallback-curves">
          {CHOIR_CURVES.map((curve, index) => (
            <path
              key={index}
              className={`choir-fallback-curve choir-fallback-curve-depth-${curve.depth}`}
              d={choirCurveToSvgPath(curve)}
              fill="none"
              stroke={curve.depth === 1 ? "#71102e" : curve.depth === 2 ? "#d8d1ca" : "#78737a"}
              strokeOpacity={0.065 + curve.depth * 0.05}
              strokeWidth={0.55 + curve.depth * 0.32}
              vectorEffect="non-scaling-stroke"
            />
          ))}
          <g className="choir-fallback-flow" fill="none">
            {CHOIR_CURVES.filter((curve) => curve.depth > 0).map((curve, index) => (
              <path
                key={index}
                d={choirCurveToSvgPath(curve)}
                stroke={curve.depth === 2 ? "#f2ece6" : "#9d1944"}
                strokeOpacity={curve.depth === 2 ? ".29" : ".19"}
                strokeWidth={curve.depth === 2 ? ".96" : ".7"}
                strokeDasharray="7 74"
                vectorEffect="non-scaling-stroke"
              />
            ))}
          </g>
        </g>
        <g className="choir-fallback-eyes">
          {CHOIR_EYES.map((eye, index) => {
            const x = eye.x * 1000;
            const y = (1 - eye.y) * 680;
            const rx = eye.rx * 1000;
            const ry = eye.ry * 680;
            return (
              <g key={index} transform={`rotate(${eye.tilt * 57.3} ${x} ${y})`}>
                <path
                  d={`M ${x - rx} ${y} Q ${x} ${y - ry * 1.8} ${x + rx} ${y} Q ${x} ${y + ry * 1.8} ${x - rx} ${y}`}
                  fill="none"
                  stroke="#ddd7d1"
                  strokeOpacity=".34"
                  strokeWidth=".8"
                />
                <circle cx={x} cy={y} r={Math.max(2, ry * 0.28)} fill="#42071b" opacity=".5" />
              </g>
            );
          })}
        </g>
        <g transform={`translate(${masterDx.toFixed(2)} ${masterDy.toFixed(2)}) rotate(${masterRoll.toFixed(2)} ${focusX} ${focusY})`}>
          <g className="choir-fallback-master-drift">
            <g className="choir-fallback-master-blink">
              <path d={masterPath} fill="url(#choir-void)" stroke="#e4ddd7" strokeOpacity=".58" strokeWidth="1.35" filter="url(#choir-soft-glow)" />
              <path d={upperFoldPath} fill="none" stroke="#c9c1bd" strokeOpacity=".23" strokeWidth="1.05" />
              <g clipPath="url(#choir-master-clip)">
                <circle
                  cx={focusX + motion.masterGazeX * 25}
                  cy={focusY - motion.masterGazeY * 18 + 4}
                  r={48 + motion.masterPulse * 7}
                  fill="url(#choir-iris)"
                  opacity={0.72 + motion.carmineDepth * 0.25}
                />
                <ellipse
                  cx={focusX + motion.masterGazeX * 25}
                  cy={focusY - motion.masterGazeY * 18 + 4}
                  rx={8.5 + motion.pupilScale * 6.5}
                  ry={25 + motion.pupilScale * 10}
                  fill="#000"
                />
                <ellipse
                  className="choir-fallback-glint"
                  cx={focusX + motion.masterGazeX * 25 - 24}
                  cy={focusY - motion.masterGazeY * 18 - 24}
                  rx="12"
                  ry="6.5"
                  fill="#f2ece6"
                  opacity=".31"
                />
                <circle
                  className="choir-fallback-glint-fine"
                  cx={focusX + motion.masterGazeX * 25 + 17}
                  cy={focusY - motion.masterGazeY * 18 - 13}
                  r="3.2"
                  fill="#d8c9ca"
                  opacity=".27"
                />
              </g>
              <path d={masterPath} fill="none" stroke="#7c0d30" strokeOpacity=".72" strokeWidth="3.8" />
            </g>
          </g>
        </g>
      </svg>
      <div className="obsession-choir-fallback-vignette absolute inset-0" />
    </div>
  );
}
