import { useId } from "react";

import { useRenderActive } from "../render";
import { CoreShell } from "./CoreShell";
import { deriveYaniMood } from "./yanineko/state";
import "./YaniTamagotchiLab.css";

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

export function YaniTamagotchiLab({
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
  const shellGradient = `${id}-shell`;
  const shellSideGradient = `${id}-shell-side`;
  const screenGradient = `${id}-screen`;
  const amberGradient = `${id}-amber`;
  const shadow = `${id}-shadow`;
  const glow = `${id}-glow`;
  const pixels = `${id}-pixels`;

  return (
    <CoreShell interactive={interactive} onClick={onClick} busy={busy} size={size}>
      <div
        aria-hidden="true"
        className="yani-tamagotchi-lab"
        data-yani-tamagotchi-lab
        data-mood={mood}
        data-motion={motionOn ? "running" : "still"}
        style={{ width: size, height: size }}
      >
        <svg viewBox="0 0 240 240" focusable="false">
          <defs>
            <linearGradient id={shellGradient} x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="#f3f1d9" />
              <stop offset="0.46" stopColor="#dce8ce" />
              <stop offset="1" stopColor="#9dbca2" />
            </linearGradient>
            <linearGradient id={shellSideGradient} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#97b49c" />
              <stop offset="1" stopColor="#5c7e67" />
            </linearGradient>
            <linearGradient id={screenGradient} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#a9c6a2" />
              <stop offset="0.55" stopColor="#91b494" />
              <stop offset="1" stopColor="#769b7e" />
            </linearGradient>
            <radialGradient id={amberGradient}>
              <stop offset="0" stopColor="#fff2bd" />
              <stop offset="0.35" stopColor="#f5a34c" />
              <stop offset="1" stopColor="#a84a28" />
            </radialGradient>
            <pattern id={pixels} width="5" height="5" patternUnits="userSpaceOnUse">
              <path d="M5 0H0V5" fill="none" stroke="#365c44" strokeOpacity=".09" strokeWidth=".45" />
            </pattern>
            <filter id={shadow} x="-35%" y="-30%" width="170%" height="200%">
              <feDropShadow dx="0" dy="9" stdDeviation="8" floodColor="#244a33" floodOpacity=".3" />
            </filter>
            <filter id={glow} x="-250%" y="-250%" width="600%" height="600%">
              <feGaussianBlur stdDeviation="2.2" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>

          <ellipse className="yani-tamagotchi-lab__halo" cx="120" cy="129" rx="91" ry="98" />
          <ellipse className="yani-tamagotchi-lab__floor" cx="121" cy="211" rx="67" ry="12" />

          <g className="yani-tamagotchi-lab__chain" fill="none" strokeLinecap="round">
            <path d="M103 44 C88 27 73 21 57 18 C44 16 39 7 48 3" />
            <circle cx="89" cy="32" r="4.2" />
            <circle cx="75" cy="25" r="4" />
            <circle cx="61" cy="20" r="3.8" />
          </g>

          <g className="yani-tamagotchi-lab__device" filter={`url(#${shadow})`}>
            <path className="yani-tamagotchi-lab__side" d="M58 75 C57 47 77 31 111 28 C153 24 181 44 184 87 L185 162 C183 190 159 207 120 209 C80 210 54 190 53 157 Z" fill={`url(#${shellSideGradient})`} />
            <path className="yani-tamagotchi-lab__shell" d="M65 69 C65 43 84 29 117 28 C156 27 177 47 177 85 L177 158 C176 184 155 199 121 201 C85 201 61 183 60 154 Z" fill={`url(#${shellGradient})`} />
            <path className="yani-tamagotchi-lab__shell-shine" d="M75 66 C77 47 91 39 116 36 C139 34 157 43 165 57" />
            <path className="yani-tamagotchi-lab__rim" d="M91 33 Q101 16 119 16 Q138 16 149 34" />
            <circle className="yani-tamagotchi-lab__ring" cx="120" cy="20" r="8" />

            <g className="yani-tamagotchi-lab__screen-frame">
              <path d="M79 61 Q121 53 164 62 L163 143 Q121 151 78 143 Z" />
              <path className="yani-tamagotchi-lab__screen" d="M86 68 Q121 62 156 68 L155 136 Q121 142 85 136 Z" fill={`url(#${screenGradient})`} />
              <path className="yani-tamagotchi-lab__screen-glare" d="M91 72 Q119 66 148 71 L147 79 Q117 74 91 80 Z" />
              <path className="yani-tamagotchi-lab__pixel-grid" d="M86 68 Q121 62 156 68 L155 136 Q121 142 85 136 Z" fill={`url(#${pixels})`} />
              <rect className="yani-tamagotchi-lab__scanline" x="86" y="72" width="69" height="7" rx="2" />
            </g>

            <g className="yani-tamagotchi-lab__pixel-yani" shapeRendering="crispEdges">
              <path className="yani-tamagotchi-lab__pixel-hair" d="M101 87h5v-10h7v5h17v-5h7v11h6v28h-5v8h-8v6h-20v-5h-8v-8h-6V93h5Z" />
              <path className="yani-tamagotchi-lab__pixel-face" d="M105 92h31v23h-5v7h-20v-5h-6Z" />
              <g className="yani-tamagotchi-lab__pixel-ear yani-tamagotchi-lab__pixel-ear--left">
                <path d="M103 89h-7V76h5v5h6v9Z" />
              </g>
              <g className="yani-tamagotchi-lab__pixel-ear yani-tamagotchi-lab__pixel-ear--right">
                <path d="M135 89h8V76h-5v5h-7v9Z" />
              </g>
              <g className="yani-tamagotchi-lab__pixel-eyes">
                <path className="yani-tamagotchi-lab__eye yani-tamagotchi-lab__eye--left" d="M109 102h8v3h-8Z" />
                <path className="yani-tamagotchi-lab__eye yani-tamagotchi-lab__eye--right" d="M125 102h8v3h-8Z" />
              </g>
              <path className="yani-tamagotchi-lab__pixel-mouth" d="M119 111h4v3h4v3h-8Z" />
              <g className="yani-tamagotchi-lab__pixel-feet">
                <path className="yani-tamagotchi-lab__pixel-foot yani-tamagotchi-lab__pixel-foot--left" d="M108 124h7v6h-9v-3h2Z" />
                <path className="yani-tamagotchi-lab__pixel-foot yani-tamagotchi-lab__pixel-foot--right" d="M127 123h7v4h3v3h-10Z" />
              </g>
              <path className="yani-tamagotchi-lab__pixel-tail" d="M137 117h6v-6h5v-11h4v16h-4v7h-11Z" />
              <g className="yani-tamagotchi-lab__cigarette">
                <path d="M99 109h-12v3h12Z" />
                <rect x="84" y="109" width="3" height="3" />
              </g>
              <g className="yani-tamagotchi-lab__zzz">
                <path d="M145 87h8v3h-4v3h4v3h-9v-3h4v-3h-3Z" />
                <path d="M151 78h6v2h-3v3h3v2h-7v-2h3v-3h-2Z" />
              </g>
              <path className="yani-tamagotchi-lab__heart" d="M148 92h4v-4h5v4h4v5h-4v4h-5v-4h-4Z" />
              <path className="yani-tamagotchi-lab__alarm" d="M150 84h5v13h-5Zm0 16h5v5h-5Z" />
            </g>

            <text x="82" y="154" className="yani-tamagotchi-lab__label">YANI PET / NO.07</text>
            <text x="146" y="154" className="yani-tamagotchi-lab__clock">03:17</text>

            <g className="yani-tamagotchi-lab__buttons">
              <circle cx="93" cy="174" r="9" />
              <circle cx="121" cy="179" r="9" />
              <circle cx="149" cy="174" r="9" />
              <circle className="yani-tamagotchi-lab__button-shine" cx="90" cy="171" r="2.1" />
              <circle className="yani-tamagotchi-lab__button-shine" cx="118" cy="176" r="2.1" />
              <circle className="yani-tamagotchi-lab__button-shine" cx="146" cy="171" r="2.1" />
            </g>
            <circle className="yani-tamagotchi-lab__led" cx="164" cy="184" r="3.5" fill={`url(#${amberGradient})`} filter={`url(#${glow})`} />

            <g className="yani-tamagotchi-lab__wear" fill="none" strokeLinecap="round">
              <path d="M72 104l-5 7 M166 116l5-3 M78 179l8 2 M153 44l8 5" />
              <path d="M151 190q7-2 12-7" />
            </g>
          </g>
        </svg>
      </div>
    </CoreShell>
  );
}
