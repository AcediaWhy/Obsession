import type { ReactNode } from "react";
import type { Tab } from "./NavRail";
import type { Theme } from "../../store/themeStore";
import { themeNavCollections } from "./ThemeNavCollections";
import "../../styles/themeNavIcons.css";

// Original 32-unit drawings: generous shapes stay readable at 22px.
// Only the selected set mounts; reactions use finite CSS animations.
const alchemist: Record<Tab, ReactNode> = {
  overview: <>
    <circle cx="16" cy="16" r="12" fill="#3e394b" />
    <circle cx="16" cy="16" r="8.5" stroke="#9c819c" />
    <g className="theme-icon-turn"><path d="m16 5 3 8 8 3-8 3-3 8-3-8-8-3 8-3Z" fill="#e5ba71" /><circle cx="16" cy="16" r="3" fill="#fff0c9" /></g>
  </>,
  dpi: <g className="theme-icon-rock">
    <path d="m10 3 12 0v4h-2v6l7 11q3 5-3 5H8q-6 0-3-5l7-11V7h-2Z" fill="#6c626f" />
    <path d="M10 19h12l4 7q1 2-3 2H9q-4 0-3-2Z" fill="#b8ccaa" stroke="none" />
    <path d="M11 7h10M9 23l-1 2" stroke="#fff0c9" />
    <circle className="theme-icon-rise" cx="17" cy="18" r="2" fill="#e7edc8" stroke="none" />
  </g>,
  ai: <>
    <path d="m16 3 9 8-4 13H11L7 11Z" fill="#a38bb9" />
    <path d="m16 3-4 9 4 12 5-12Z" fill="#dac5e4" />
    <path d="M7 11h18M5 28h22m-19-4 3 4m13-4-3 4" />
    <path className="theme-icon-glint" d="m25 2 1 3 3 1-3 1-1 3-1-3-3-1 3-1Z" fill="#fff0c9" stroke="none" />
  </>,
  telegram: <g className="theme-icon-float">
    <path d="m3 10 13-7 13 7v17H3Z" fill="#c79664" />
    <path d="M3 11h26v16H3Z" fill="#efdbb4" />
    <path d="m3 11 13 10 13-10M3 27l9-9m17 9-9-9" stroke="#926b58" />
    <circle cx="16" cy="20" r="4" fill="#ad737a" stroke="#623f51" />
    <path d="m16 17 1 3-1 2-1-2Z" fill="#f8dfb0" stroke="none" />
  </g>,
  lists: <>
    <path d="M6 5h21v24H7q-3 0-3-3V8Z" fill="#efdbb4" />
    <path d="M8 26h18" stroke="#967366" />
    <g className="theme-icon-page"><path d="M7 3h20v21H7q-3 0-3 3V6q0-3 3-3Z" fill="#66516d" /><path d="M9 4v19" /><path d="m18 7 5 9H13Z" fill="#dfb979" /><circle cx="18" cy="12" r="2" fill="#66516d" stroke="none" /></g>
  </>,
  profiles: <g className="theme-icon-rock">
    <path d="M6 4h6v4H6Zm14 2h6v4h-6Z" fill="#c28f64" />
    <path d="M6 8h6v4q4 2 4 7v7q0 3-3 3H5q-3 0-3-3v-7q0-5 4-7Z" fill="#9b8daf" />
    <path d="M20 10h6v4q4 2 4 7v5q0 3-3 3h-8q-2 0-2-3v-5q0-5 3-7Z" fill="#adc5b6" />
    <path d="M3 21h12m3 1h11" stroke="#efdbb4" /><path d="m9 15 2 3-2 3-2-3Z" fill="#efdbb4" stroke="none" />
  </g>,
  settings: <>
    <path className="theme-icon-stir" d="m17 18 9-14q1-2 3 0l-8 15Z" fill="#dbc49d" />
    <path d="M3 15h26q-1 11-11 12h-4Q4 26 3 15Z" fill="#9c849d" />
    <path d="M3 15q13 6 26 0M9 29h14" /><path d="M7 20q2 4 6 4" stroke="#e9d5e7" />
  </>,
};

const rain: Record<Tab, ReactNode> = {
  overview: <>
    <path d="M8 21a6 6 0 0 1-1-12 8 8 0 0 1 15-1 6.5 6.5 0 0 1 2 13Z" fill="#d9e6ed" />
    <path d="M10 7q4-3 7 0" stroke="#fff9e9" />
    <g className="theme-icon-drizzle" stroke="#89c7e2" strokeWidth="2.5"><path d="m9 25-1 3m9-3-1 3m9-3-1 3" /></g>
  </>,
  dpi: <g className="theme-icon-rock">
    <path d="M16 3v23q0 5-5 3l-1-2" stroke="#dce9ee" strokeWidth="2" />
    <path d="M2 18Q4 5 16 5t14 13q-5-4-9 0-5-4-10 0-4-4-9 0Z" fill="#9bbaca" />
    <path d="M11 18q0-9 5-13 5 4 5 13-5-4-10 0Z" fill="#e7d5bf" />
  </g>,
  ai: <>
    <path className="theme-icon-float" d="M16 3Q4 16 5 21a11 9 0 0 0 22 0Q28 16 16 3Z" fill="#89bfd3" />
    <path className="theme-icon-glint" d="m17 13 1.5 4.5L23 19l-4.5 1.5L17 25l-1.5-4.5L11 19l4.5-1.5Z" fill="#eff8f6" stroke="none" />
    <path d="M9 17q-3 5 0 7" stroke="#cde9ed" />
  </>,
  telegram: <>
    <g className="theme-icon-sail"><path d="m3 17 13-11 13 11Z" fill="#f2e7d2" /><path d="M16 6v17L3 17m13 6 13-6" /><path d="m2 17 7 9h14l7-9-14 6Z" fill="#b7d3df" /></g>
    <path d="M3 29q3-2 6 0t6 0 6 0 6 0" stroke="#8caec4" />
  </>,
  lists: <>
    <path d="M7 4h20v25H7q-3 0-3-3V7q0-3 3-3Z" fill="#edf0e6" />
    <g className="theme-icon-page"><path d="M7 3h19v22H7q-3 0-3 3V6q0-3 3-3Z" fill="#91abbf" /><path d="M9 4v20" /><path d="M13 9h9m-9 5h9m-9 5h6" stroke="#eff5ee" /></g>
    <path d="M20 3v8l-2-2-2 2V3Z" fill="#dbb997" stroke="none" />
  </>,
  profiles: <g className="theme-icon-rock">
    <path d="M10 8V6a6 6 0 0 1 12 0v2" fill="none" stroke="#a0b8ca" />
    <path d="m4 9 11-2 12 9-11 14L3 20Z" fill="#97b9c6" />
    <path d="m11 8 13 1 5 15-15 5-8-15Z" fill="#d9dfd8" />
    <circle cx="15" cy="13" r="2" fill="#53677f" stroke="none" />
    <path d="m16 19 7-2m-6 6 7-2" stroke="#839eb2" />
  </g>,
  settings: <>
    <path d="M18 3v25M8 3v25m17-25v25" stroke="#aac2d2" strokeWidth="2" />
    <g className="theme-icon-slide"><path d="M8 8q-6 6-5 8a5 5 0 0 0 10 0Q14 14 8 8Z" fill="#aacddd" /></g>
    <circle cx="18" cy="9" r="3.5" fill="#e5d5bb" />
    <circle cx="25" cy="22" r="3.5" fill="#b7c7dc" />
  </>,
};

const collections: Record<Exclude<Theme, "goldenmeadow">, Record<Tab, ReactNode>> = {
  ...themeNavCollections, ophanim: alchemist, japan: rain,
};

export function ThemeNavIcon({ theme, item }: { theme: Exclude<Theme, "goldenmeadow">; item: Tab }) {
  return (
    <svg className={`theme-nav-icon theme-nav-icon-${theme}`} width="22" height="22" viewBox="0 0 32 32"
      aria-hidden="true" focusable="false" fill="none" stroke={theme === "ophanim" ? "#d6af77" : theme === "japan" ? "#4a627b" : "var(--nav-icon-ink)"}
      strokeWidth="1.4" strokeLinejoin="round" strokeLinecap="round">
      {collections[theme][item]}
    </svg>
  );
}
