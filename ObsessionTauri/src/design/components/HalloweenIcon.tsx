import type { ReactNode } from "react";
import type { Tab } from "./NavRail";
import "../../styles/halloweenIcons.css";

// Original small SVG drawings. Keep moving parts separate and colors shared;
// no image decoders, external downloads, animation player or idle timers.
const ink = "#43343f";
const cream = "#fff0d5";
const gold = "#f7cc70";
const orange = "#efa15d";
const violet = "#b8a0ce";
const leaf = "#b6c886";

const pictures: Record<Tab, ReactNode> = {
  overview: <g className="halloween-candy">
    <path d="M12.3 6.5q3.7-7 7.4 0l9 18.4q2.4 5.1-4 5.1H7.3q-6.4 0-4-5.1Z" fill={gold} />
    <path d="m8.9 13.5 3.4-7q3.7-7 7.4 0l3.4 7Z" fill={cream} />
    <path d="m8.9 13.5-5 10h24.2l-5-10Z" fill={orange} />
    <path d="M12.3 6.5q3.7-7 7.4 0l9 18.4q2.4 5.1-4 5.1H7.3q-6.4 0-4-5.1Z" fill="none" />
    <path d="m11 16-3 6" stroke={cream} opacity=".45" />
    <ellipse cx="17" cy="8.5" rx="1.4" ry="2" fill="#fff" stroke="none" />
  </g>,
  dpi: <g className="halloween-ghost">
    <path d="M6 26V14C6 1 26 1 26 14v13q-3-5-6 0-4-5-8 0-3-5-6-1Z" fill={cream} />
    <path d="M8 15v8" stroke="#fff" strokeWidth="2" />
    <ellipse cx="12" cy="14" rx="1.5" ry="2" fill={ink} stroke="none" />
    <ellipse cx="20" cy="14" rx="1.5" ry="2" fill={ink} stroke="none" />
    <path d="M14 19q2 2 4 0" fill="none" />
    <path d="m9 18 1 .3m12-.3 1-.3" stroke={orange} strokeWidth="2" />
  </g>,
  ai: <>
    <circle cx="16" cy="13" r="10.5" fill={violet} />
    <path d="M10 7q-4 4-2 8" stroke={cream} strokeWidth="2" fill="none" />
    <path d="M11 23h10l4 6H7Z" fill={gold} />
    <path d="M8 29h16" />
    <path className="halloween-spark" d="m18 7 1.3 4.5L24 13l-4.7 1.5L18 19l-1.3-4.5L12 13l4.7-1.5Z" fill={cream} stroke="none" />
  </>,
  telegram: <>
    <path className="halloween-wing-left" d="M13 15Q7 7 1 6Q4 12 1 19Q5 16 7 22Q10 18 13 23Z" fill={violet} />
    <path className="halloween-wing-right" d="M19 15Q25 7 31 6Q28 12 31 19Q27 16 25 22Q22 18 19 23Z" fill={violet} />
    <path className="halloween-bat-body" d="M12 15 12 8 16 12 20 8 20 15Q23 21 16 25Q9 21 12 15Z" fill="#69526f" />
    <circle cx="14" cy="17" r="1.25" fill={cream} stroke="none" />
    <circle cx="18" cy="17" r="1.25" fill={cream} stroke="none" />
  </>,
  lists: <>
    <path d="M6 5h20v23H7q-3 0-3-3V8q0-3 2-3Z" fill={cream} />
    <path d="M8 25h17m-17 3v-3" fill="none" />
    <g className="halloween-book-cover">
      <path d="M7 3h19v21H7q-3 0-3 3V6q0-3 3-3Z" fill={orange} />
      <path d="M8 4v19" stroke={gold} strokeWidth="2" />
      <path d="m18 7 1.3 4 4.2 1.5-4.2 1.3-1.3 4.1-1.4-4.1-4.1-1.3 4.1-1.5Z" fill={cream} stroke="none" />
    </g>
  </>,
  profiles: <g className="halloween-mask">
    <path d="M3 8q13-6 26 0l-2 13q-4 8-11 2-7 6-11-2Z" fill={cream} />
    <path d="M16 5q7 0 13 3l-2 13q-4 8-11 2Z" fill={violet} />
    <path d="M3 8q13-6 26 0l-2 13q-4 8-11 2-7 6-11-2Z" fill="none" />
    <path d="M7 13q4-3 6 2-4 3-6-2Zm12 2q2-5 6-2-2 5-6 2Z" fill={ink} stroke="none" />
    <path d="m13 19 3-2 3 2" fill="none" />
  </g>,
  settings: <>
    <g className="halloween-bubbles" fill={leaf}>
      <circle className="halloween-bubble-one" cx="13" cy="8" r="2.5" />
      <circle className="halloween-bubble-two" cx="21" cy="5" r="2" />
    </g>
    <path d="m9 26-2 4m16-4 2 4" fill="none" strokeWidth="2.5" />
    <path d="M7 13q-9 15 9 15t9-15Z" fill="#79657f" />
    <ellipse cx="16" cy="13" rx="10" ry="3" fill={leaf} />
    <path d="M7 17q-3 7 4 8" fill="none" stroke={violet} />
    <path d="m18 18 .8 2.3 2.2.7-2.2.8L18 24l-.8-2.2-2.2-.8 2.2-.7Z" fill={gold} stroke="none" />
  </>,
};

export function HalloweenIcon({ item }: { item: Tab }) {
  return (
    <svg className={`halloween-icon halloween-icon-${item}`} width="22" height="22" viewBox="0 0 32 32"
      aria-hidden="true" focusable="false" fill="none" stroke={ink} strokeWidth="1.4" strokeLinejoin="round" strokeLinecap="round">
      {pictures[item]}
    </svg>
  );
}
