import { useId, type CSSProperties } from "react";
import ruinsGarden from "../../assets/fallendown/ruins-garden-v3.png";

// Deterministic positions: changing protection or motion never reseeds the dust.
const dust = Array.from({ length: 28 }, (_, index) => {
  const y = 40 + ((index * 137) % 670);
  return {
    x: 1326 - y * .24 + (((index * 53) % 150) - 75),
    y,
    size: index % 5 === 0 ? 2.6 : 1.6,
    duration: 13 + (index % 7) * 1.7,
    delay: -(index * 2.3),
    drift: -12 - (index % 4) * 6,
  };
});

const embers = Array.from({ length: 12 }, (_, index) => ({
  x: 808 + (index * 137) % 540,
  y: 690 + (index * 29) % 90,
  duration: 9 + index % 5 * 1.6,
  delay: -index * 2.7,
  drift: index % 2 ? 22 : -30,
  rise: 75 + index % 4 * 18,
}));

const fireflies = Array.from({ length: 9 }, (_, index) => ({
  x: 770 + (index * 173) % 650,
  y: 585 + (index * 47) % 185,
  duration: 17 + (index % 4) * 3,
  pulse: 5.8 + (index % 3) * 1.9,
  delay: -index * 3.7,
  drift: index % 2 ? 34 : -42,
  lift: -18 - (index % 4) * 9,
}));

// Local artwork, mounted only with the visible Fallen Down scene.
// The save light stays a separate state-driven element; no extra render loop.
export function FallenRuins() {
  const atmosphereId = useId();
  return (
    <div className="fallen-ruins" aria-hidden="true">
      <div className="fallen-room-frame">
        <div className="fallen-room-plane">
          <img className="fallen-room-art" src={ruinsGarden} alt="" width="1536" height="1024" draggable={false} decoding="async" />
          <svg className="fallen-atmosphere" viewBox="0 0 1536 1024" focusable="false">
            <defs>
              <linearGradient id={`${atmosphereId}-beam`} x1="0" y1="0" x2="0" y2="1" gradientUnits="objectBoundingBox">
                <stop stopColor="#fff0c9" stopOpacity=".22" />
                <stop offset=".55" stopColor="#ffe0a0" stopOpacity=".11" />
                <stop offset="1" stopColor="#ffd479" stopOpacity="0" />
              </linearGradient>
              <radialGradient id={`${atmosphereId}-warm`}>
                <stop stopColor="#ffe7aa" stopOpacity=".22" />
                <stop offset=".45" stopColor="#efbc65" stopOpacity=".09" />
                <stop offset="1" stopColor="#efbc65" stopOpacity="0" />
              </radialGradient>
              <radialGradient id={`${atmosphereId}-mist`}>
                <stop stopColor="#beb0d8" stopOpacity=".13" />
                <stop offset=".5" stopColor="#a79bc7" stopOpacity=".05" />
                <stop offset="1" stopColor="#a79bc7" stopOpacity="0" />
              </radialGradient>
              <radialGradient id={`${atmosphereId}-spark`}>
                <stop stopColor="#ffe6a1" stopOpacity=".65" />
                <stop offset=".3" stopColor="#ffce73" stopOpacity=".16" />
                <stop offset="1" stopColor="#ffce73" stopOpacity="0" />
              </radialGradient>
              <radialGradient id={`${atmosphereId}-ray`}>
                <stop stopColor="#fff3d0" stopOpacity=".2" />
                <stop offset=".38" stopColor="#ffe6b0" stopOpacity=".08" />
                <stop offset="1" stopColor="#ffe6b0" stopOpacity="0" />
              </radialGradient>
              <radialGradient id={`${atmosphereId}-firefly`}>
                <stop stopColor="#ffffd5" stopOpacity=".85" />
                <stop offset=".16" stopColor="#ffe58a" stopOpacity=".42" />
                <stop offset=".5" stopColor="#e8b85f" stopOpacity=".12" />
                <stop offset="1" stopColor="#e8b85f" stopOpacity="0" />
              </radialGradient>
            </defs>
            <g fill={`url(#${atmosphereId}-ray)`}>
              <g transform="rotate(20 1250 340)">
                <ellipse className="fallen-ray" cx="1250" cy="340" rx="46" ry="490" />
              </g>
              <g transform="rotate(24 1350 350)">
                <ellipse className="fallen-ray fallen-ray-secondary" cx="1350" cy="350" rx="25" ry="440" />
              </g>
            </g>
            <g className="fallen-light-breath">
              <path d="M1300 0h67l-123 710H968Z" fill={`url(#${atmosphereId}-beam)`} />
              <path d="M1335 0h13l-119 729h-105Z" fill={`url(#${atmosphereId}-beam)`} opacity=".65" />
              <ellipse cx="1150" cy="690" rx="300" ry="160" fill={`url(#${atmosphereId}-warm)`} />
            </g>
            <ellipse className="fallen-floor-glow" cx="1080" cy="780" rx="285" ry="82" fill={`url(#${atmosphereId}-warm)`} />
            <g className="fallen-floor-mist" fill={`url(#${atmosphereId}-mist)`}>
              <ellipse cx="765" cy="815" rx="420" ry="62" />
              <ellipse cx="1230" cy="865" rx="330" ry="46" />
            </g>
            <ellipse className="fallen-floor-mist fallen-floor-mist-distant" cx="775" cy="573" rx="360" ry="36" fill={`url(#${atmosphereId}-mist)`} />
            <g className="fallen-foreground-mist" fill={`url(#${atmosphereId}-mist)`}>
              <ellipse cx="815" cy="927" rx="400" ry="83" />
              <ellipse cx="1390" cy="966" rx="340" ry="67" />
            </g>
            <g fill="#ffe5a2">
              {dust.map((particle, index) => (
                <circle key={index} className="fallen-dust" cx={particle.x} cy={particle.y} r={particle.size}
                  style={{ "--dust-time": `${particle.duration}s`, "--dust-delay": `${particle.delay}s`, "--dust-drift": `${particle.drift}px` } as CSSProperties} />
              ))}
            </g>
            {embers.map((ember, index) => (
              <g key={index} transform={`translate(${ember.x} ${ember.y})`}>
                <g className="fallen-ember" style={{
                  "--ember-time": `${ember.duration}s`, "--ember-delay": `${ember.delay}s`,
                  "--ember-drift": `${ember.drift}px`, "--ember-rise": `${-ember.rise}px`,
                } as CSSProperties}>
                  <circle r="12" fill={`url(#${atmosphereId}-spark)`} />
                  <path d="M-1-4h2v3h3v2H1v3h-2V1h-3v-2h3Z" fill="#ffe4a0" opacity={index % 3 === 0 ? .9 : .5} />
                  <rect x="-1" y="-1" width="2" height="2" fill="#fff5d9" />
                </g>
              </g>
            ))}
            {fireflies.map((firefly, index) => (
              <g key={index} transform={`translate(${firefly.x} ${firefly.y})`}>
                <g className="fallen-firefly" style={{
                  "--fly-time": `${firefly.duration}s`, "--fly-pulse": `${firefly.pulse}s`,
                  "--fly-delay": `${firefly.delay}s`, "--fly-drift": `${firefly.drift}px`,
                  "--fly-lift": `${firefly.lift}px`,
                } as CSSProperties}>
                  <g className="fallen-firefly-light">
                    <circle r="17" fill={`url(#${atmosphereId}-firefly)`} />
                    <rect x="-1.25" y="-1.25" width="2.5" height="2.5" fill="#fff5ba" />
                  </g>
                </g>
              </g>
            ))}
          </svg>
        </div>
      </div>
      <svg className="fallen-save-light" viewBox="0 0 32 32" focusable="false" shapeRendering="crispEdges">
        <g className="fallen-save-shimmer">
          <path d="M14 0h4v9h4v5h10v4H22v5h-4v9h-4v-9h-4v-5H0v-4h10V9h4Z" fill="#edc966" />
          <path d="M14 9h4v14h-4ZM9 14h14v4H9Z" fill="#fff5cb" />
        </g>
        <g className="fallen-save-flash" fill="#fff5cb">
          <path d="M14 0h4v9h4v5h10v4H22v5h-4v9h-4v-9h-4v-5H0v-4h10V9h4Z" />
        </g>
      </svg>
    </div>
  );
}
