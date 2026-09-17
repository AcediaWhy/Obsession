import { useId } from "react";

import { useRenderActive } from "../render";
import { CoreShell } from "./CoreShell";
import { deriveYaniMood } from "./yanineko/state";
import "./YaniNodeCore.css";

type Props = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  onClick: () => void;
  size?: number;
  paused?: boolean;
  interactive?: boolean;
};

export function YaniNodeCore({
  active,
  busy = false,
  scanning = false,
  alarm = false,
  onClick,
  size = 240,
  paused = false,
  interactive = true,
}: Props) {
  const mood = deriveYaniMood({ active, busy, scanning, alarm });
  const motionOn = useRenderActive() && !paused;
  const id = useId().replace(/:/g, "");
  const topGradient = `${id}-top`;
  const frontGradient = `${id}-front`;
  const antennaGradient = `${id}-antenna`;
  const orangeLed = `${id}-orange`;
  const greenLed = `${id}-green`;
  const shadow = `${id}-shadow`;
  const ledGlow = `${id}-led-glow`;

  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <div
        aria-hidden="true"
        className="yani-node-core"
        data-yani-node-core
        data-mood={mood}
        data-motion={motionOn ? "running" : "still"}
        style={{ width: size, height: size }}
      >
        <svg viewBox="0 0 240 240" focusable="false">
          <defs>
            <linearGradient id={topGradient} x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="#f0f5e1" />
              <stop offset="0.52" stopColor="#cfdfc1" />
              <stop offset="1" stopColor="#96b798" />
            </linearGradient>
            <linearGradient id={frontGradient} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#9db89d" />
              <stop offset="1" stopColor="#4f715b" />
            </linearGradient>
            <linearGradient id={antennaGradient} x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="#496c58" />
              <stop offset="1" stopColor="#153225" />
            </linearGradient>
            <radialGradient id={orangeLed}>
              <stop offset="0" stopColor="#fff1b6" />
              <stop offset="0.32" stopColor="#f5a14b" />
              <stop offset="1" stopColor="#963d24" />
            </radialGradient>
            <radialGradient id={greenLed}>
              <stop offset="0" stopColor="#efffdc" />
              <stop offset="0.36" stopColor="#86d08d" />
              <stop offset="1" stopColor="#2f6d48" />
            </radialGradient>
            <filter id={shadow} x="-30%" y="-30%" width="160%" height="190%">
              <feDropShadow dx="0" dy="8" stdDeviation="7" floodColor="#183b28" floodOpacity="0.32" />
            </filter>
            <filter id={ledGlow} x="-300%" y="-300%" width="700%" height="700%">
              <feGaussianBlur stdDeviation="2.4" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>

          <ellipse className="yani-node-core__halo" cx="120" cy="133" rx="102" ry="91" />
          <ellipse className="yani-node-core__floor" cx="121" cy="190" rx="91" ry="15" />

          <g className="yani-node-core__cable" fill="none" strokeLinecap="round">
            <path d="M193 139 C220 125 230 132 247 110" />
            <path className="yani-node-core__packet" d="M194 137 C220 124 232 132 247 110" />
          </g>

          <g className="yani-node-core__antenna yani-node-core__antenna--left">
            <path d="M58 105 C55 77 51 39 59 16 C77 36 89 69 92 101 Z" fill={`url(#${antennaGradient})`} />
            <path d="M64 91 C62 67 62 45 64 34 C75 49 83 70 86 93 Z" className="yani-node-core__antenna-inner" />
            <path d="M68 82 C67 65 67 53 67 45" className="yani-node-core__antenna-vein" />
          </g>
          <g className="yani-node-core__antenna yani-node-core__antenna--right">
            <path d="M151 99 C160 67 170 37 189 14 C192 48 182 79 176 106 Z" fill={`url(#${antennaGradient})`} />
            <path d="M160 93 C166 70 174 48 184 31 C184 55 176 79 170 96 Z" className="yani-node-core__antenna-inner" />
            <path d="M166 84 C171 67 176 54 181 44" className="yani-node-core__antenna-vein" />
          </g>

          <g className="yani-node-core__router" filter={`url(#${shadow})`}>
            <path d="M42 105 L183 99 L213 128 L32 139 Z" fill={`url(#${topGradient})`} />
            <path d="M32 139 L213 128 L209 169 L36 179 Z" fill={`url(#${frontGradient})`} />
            <path d="M42 105 L32 139 L36 179 L43 147 Z" className="yani-node-core__side" />

            <g className="yani-node-core__vents">
              <path d="M67 117 L88 116 M96 115 L117 114 M125 113 L146 112 M154 111 L175 110" />
              <path d="M75 125 L96 124 M104 123 L125 122 M133 121 L154 120 M162 119 L183 118" />
            </g>

            <text x="53" y="157" className="yani-node-core__brand">YANI NODE</text>
            <text x="169" y="153" className="yani-node-core__budget">170円</text>

            <g className="yani-node-core__leds" filter={`url(#${ledGlow})`}>
              <circle className="yani-node-core__led yani-node-core__led--main" cx="103" cy="160" r="4" fill={`url(#${orangeLed})`} />
              <circle className="yani-node-core__led yani-node-core__led--one" cx="119" cy="159" r="2.6" fill={`url(#${greenLed})`} />
              <circle className="yani-node-core__led yani-node-core__led--two" cx="131" cy="158.5" r="2.6" fill={`url(#${greenLed})`} />
              <circle className="yani-node-core__led yani-node-core__led--three" cx="143" cy="158" r="2.6" fill={`url(#${greenLed})`} />
              <circle className="yani-node-core__led yani-node-core__led--four" cx="155" cy="157.5" r="2.6" fill={`url(#${greenLed})`} />
            </g>

            <g className="yani-node-core__wear">
              <path d="M50 148 l13 -1 M183 143 l16 -1 M185 150 l8 -.5" />
              <path d="M47 123 q8 -6 16 -2" />
            </g>
            <ellipse className="yani-node-core__burn" cx="190" cy="122" rx="6.5" ry="3.4" />
            <ellipse className="yani-node-core__burn-center" cx="190" cy="121.6" rx="3.4" ry="1.5" />
          </g>

          <g className="yani-node-core__signal" fill="none" strokeLinecap="round">
            <path d="M101 82 Q120 65 139 81" />
            <path d="M109 91 Q120 82 131 90" />
            <circle cx="120" cy="96" r="2.3" />
          </g>
        </svg>
      </div>
    </CoreShell>
  );
}
