import type { ReactNode } from "react";
import type { Tab } from "./NavRail";
import type { Theme } from "../../store/themeStore";

// Общая геометрия и отдельные рисунки для каждой темы. Палитра берётся из SVG,
// чтобы значки оставались различимыми на поверхности меню.
const paper = "var(--nav-icon-paper)";
const color = "var(--nav-icon-color)";
const shade = "var(--nav-icon-shade)";
const ink = "var(--nav-icon-ink)";
type CollectionTheme = Exclude<Theme, "goldenmeadow" | "ophanim" | "japan">;

export const themeNavCollections: Record<CollectionTheme, Record<Tab, ReactNode>> = {
  aurora: {
    overview: <>
      <path className="theme-icon-sail" d="M3 7q5 6 10 0t16 0v12q-8-6-16 1T3 20Z" fill={color} />
      <path d="M4 12q5 5 11-1t13 1" stroke={paper} /><path d="m2 28 8-11 7 11 6-8 7 8Z" fill={shade} />
    </>,
    dpi: <g className="theme-icon-rock">
      <path d="m16 2 11 7-4 16-7 5-7-5L5 9Z" fill={color} /><path d="m16 2-5 9 5 19 5-19Z" fill={paper} /><path d="M5 9h22M11 11h10" />
    </g>,
    ai: <>
      <circle cx="16" cy="16" r="8" fill={color} /><ellipse className="theme-icon-orbit" cx="16" cy="16" rx="15" ry="5" stroke={paper} strokeWidth="2" />
      <circle cx="13" cy="13" r="2" fill={paper} stroke="none" />
    </>,
    telegram: <g className="theme-icon-float">
      <path d="m3 27 10-14m-4 16 7-11M2 20l8-5" stroke={color} strokeWidth="2.5" />
      <path d="m22 2 2 7 7 2-6 5v7l-7-4-7 2 2-8-4-5 8-1Z" fill={paper} />
    </g>,
    lists: <g className="theme-icon-page">
      <path d="m3 7 9-3 9 3 8-3v23l-8 3-9-3-9 3Z" fill={shade} /><path d="M12 4v23m9-20v23" stroke={color} />
      <path d="m7 20 7-8 10 6" stroke={paper} /><g fill={paper} stroke="none"><circle cx="7" cy="20" r="2" /><circle cx="14" cy="12" r="2" /><circle cx="24" cy="18" r="2" /></g>
    </g>,
    profiles: <g className="theme-icon-rock">
      <path d="M3 8h20v20H3Z" fill={shade} /><path d="M9 3h20v20H9Z" fill={color} /><circle cx="19" cy="10" r="3" fill={paper} stroke="none" /><path d="m12 20 5-6 4 4 3-3 3 5Z" fill={paper} stroke="none" />
    </g>,
    settings: <>
      <circle cx="16" cy="16" r="12" fill={shade} /><path d="M16 5v3m11 8h-3M16 27v-3M5 16h3" stroke={color} />
      <g className="theme-icon-rock"><path d="m20 7-1 12-7 6 1-12Z" fill={paper} /><path d="m20 7-1 12-6-6Z" fill={color} /></g>
    </>,
  },
  midnight: {
    overview: <g className="theme-icon-rock">
      <path d="M12 6V4a4 4 0 0 1 8 0v2M6 10l6-4h8l6 4-3 17H9ZM7 29h18" fill={shade} />
      <path className="theme-icon-glint" d="M11 12h10l-2 12h-6Z" fill={paper} stroke="none" /><path d="M6 10h20M16 11v14" />
    </g>,
    dpi: <g className="theme-icon-float"><path d="M24 3A13 13 0 1 0 29 22C13 28 8 10 24 3Z" fill={paper} /><path d="m25 8 1 3 3 1-3 1-1 3-1-3-3-1 3-1Z" fill={color} stroke="none" /></g>,
    ai: <g className="theme-icon-rock">
      <path d="m3 13 19-9 5 9-19 9Z" fill={color} /><path d="m20 4 5-2 5 10-5 2Z" fill={paper} /><path d="m16 19-6 11m6-11 7 11m-7-11v10" stroke={paper} strokeWidth="2" />
    </g>,
    telegram: <>
      <path d="M6 28V12q0-7 7-7h10q6 0 6 7v12H6" fill={shade} /><path d="M6 14h23M13 5q6 0 6 8" />
      <g className="theme-icon-float"><path d="M2 14h18v12H2Z" fill={paper} /><path d="m2 14 9 7 9-7" stroke={shade} /></g>
    </>,
    lists: <g className="theme-icon-page">
      <path d="M4 6h9v24H4Zm11-3h7v27h-7Zm9 6h5v21h-5Z" fill={color} /><path d="M6 10h5m-5 14h5m6-17h3m-3 18h3m6-12h1" stroke={paper} />
    </g>,
    profiles: <g className="theme-icon-rock">
      <path d="m4 6 21-3 3 23-21 3Z" fill={shade} /><path d="M3 8h20v21H3Z" fill={paper} /><circle cx="13" cy="15" r="3" fill={shade} /><path d="M7 25a6 6 0 0 1 12 0" fill={color} />
    </g>,
    settings: <>
      <path d="M13 2h6v4h-6Z" fill={color} /><circle cx="16" cy="18" r="11" fill={paper} /><circle cx="16" cy="18" r="8" fill={shade} />
      <path className="theme-icon-clock" d="M16 11v7l4 2" stroke={paper} strokeWidth="2" />
    </>,
  },
  catnap: {
    overview: <g className="theme-icon-rock">
      <path d="m5 12 1-9 8 6h5l7-6 1 11q5 15-11 15T5 12Z" fill={color} /><path d="m8 18 3 2 3-2m5 0 3 2 3-2" stroke={shade} strokeWidth="2" /><path d="m15 23 2 2 2-2" fill={shade} stroke="none" />
    </g>,
    dpi: <g className="theme-icon-sail">
      <path d="M6 3h20v23H6Z" fill={color} /><path d="M9 7h14v10H9Z" fill={shade} /><path d="M16 7v10" stroke={paper} /><circle cx="10" cy="22" r="2" fill={paper} stroke="none" /><circle cx="22" cy="22" r="2" fill={paper} stroke="none" /><path d="m10 26-4 4m16-4 4 4" stroke={paper} strokeWidth="2" />
    </g>,
    ai: <>
      <path d="M24 13h3q6 8-3 10M4 12h20v10q0 6-10 6T4 22Z" fill={paper} /><path d="M3 29h24" />
      <path className="theme-icon-steam" d="M10 9q-4-3 0-6m7 6q-4-3 0-6" stroke={color} strokeWidth="2" />
    </>,
    telegram: <g className="theme-icon-float">
      <path d="M2 7h28v20H2Z" fill={paper} /><path d="M18 10v14" /><path d="M22 11h5v6h-5Z" fill={color} /><circle cx="10" cy="14" r="3" fill={color} stroke="none" /><path d="m4 23 6-5 5 5m6-2h6m-6 3h4" />
    </g>,
    lists: <g className="theme-icon-page">
      <path d="M3 5h26v6q-5 3 0 6v10H3V17q5-3 0-6Z" fill={paper} /><path d="M21 7v18" strokeDasharray="2 3" /><path d="M7 10h9m-9 5h9m-9 5h5" stroke={shade} />
    </g>,
    profiles: <g className="theme-icon-rock catnap-paw" fill={paper} stroke="none">
      <ellipse cx="6" cy="13" rx="3" ry="4" />
      <ellipse cx="12" cy="7.5" rx="3" ry="4" />
      <ellipse cx="20" cy="7.5" rx="3" ry="4" />
      <ellipse cx="26" cy="13" rx="3" ry="4" />
      <path d="M16 15c-3.5 0-4.5 4-7.5 7-2.5 2.5-2 6 1.5 6 2.5 0 4-1.5 6-1.5s3.5 1.5 6 1.5c3.5 0 4-3.5 1.5-6-3-3-4-7-7.5-7Z" />
    </g>,
    settings: <g className="theme-icon-rock">
      <circle cx="15" cy="15" r="11" fill={color} /><path d="M7 8q12 0 18 12M4 16q9-9 16-11M7 23q3-8 17-10M12 26q11-2 14 2t4-4" stroke={paper} />
    </g>,
  },
  fallendown: {
    overview: <g className="theme-icon-beat" fill={color} stroke="none"><path d="M4 5h8v4h8V5h8v12h-4v4h-4v4h-4v4h-4v-4H8v-4H4v-4H0V9h4Z" transform="translate(2 0) scale(.875 1)" /></g>,
    dpi: <g className="theme-icon-rock" fill={paper} stroke="none"><path d="M18 2h10v10h-4v4h-4v4h-4l-4 4-4-4 4-4v-4h4V8h2Z" /><path d="M4 18h4v4h4v4H8v4H2v-6h4v-2H4Z" fill={color} /></g>,
    ai: <g className="theme-icon-glint" fill={paper} stroke="none"><path d="M14 2h4v8h4v4h8v4h-8v4h-4v8h-4v-8h-4v-4H2v-4h8v-4h4Z" /></g>,
    telegram: <g className="theme-icon-float" fill={paper} stroke="none"><path d="M2 6h28v20H2Z" /><path d="M6 10h4v4h4v4h4v-4h4v-4h4v4h-4v4h-4v4h-4v-4h-4v-4H6Z" fill={shade} /></g>,
    lists: <g className="theme-icon-page" fill={paper} stroke="none"><path d="M4 3h22v4h4v22H4Z" /><path d="M8 3h4v26H8Z" fill={color} /><path d="M16 9h10v4H16Zm0 8h8v4h-8Z" fill={shade} /></g>,
    profiles: <g className="theme-icon-rock" stroke="none"><path d="M2 8h20v22H2Z" fill={ink} /><path d="M10 2h20v22H10Z" fill={paper} /><path d="M16 6h8v8h-8Zm-2 10h12v4H14Z" fill={shade} /></g>,
    settings: <g className="theme-icon-slide" fill={paper} stroke="none"><path d="M6 2h4v28H6Zm16 0h4v28h-4Z" /><path d="M2 8h12v8H2Zm16 10h12v8H18Z" fill={color} /></g>,
  },
  yanineko: {
    overview: <g className="theme-icon-rock">
      <path d="m5 13 0-10 9 6h5l8-6v11q4 14-11 14T5 13Z" fill={paper} /><path d="m8 17 5 1m7 0 5-1" stroke={ink} strokeWidth="2" /><path d="m14 23 2 1 2-1" /><path d="m21 26 8-3" stroke={color} strokeWidth="3" />
    </g>,
    dpi: <>
      <path d="M7 13h18v17H7Z" fill={color} /><path d="M7 18h18M11 25h3" stroke={paper} />
      <path className="theme-icon-flicker" d="M16 1q-9 7-4 11 9 4 9-5l-4 3Z" fill={paper} />
    </>,
    ai: <>
      <path d="M3 14h26q-2 14-13 15Q4 28 3 14Z" fill={paper} /><path d="M8 19h16" stroke={color} strokeWidth="3" />
      <path d="m8 3 10 11m-2-12 8 12" stroke={ink} strokeWidth="2" /><path className="theme-icon-steam" d="M5 11q-3-3 0-5" stroke={color} />
    </>,
    telegram: <g className="theme-icon-rock">
      <path d="M9 2h15q3 0 3 3v24H6V5q0-3 3-3Z" fill={shade} /><path d="M9 6h15v17H9Z" fill={paper} /><path d="M12 11h9v6h-5l-3 3v-3h-1Z" fill={color} stroke="none" /><path d="M14 26h5" stroke={paper} />
    </g>,
    lists: <g className="theme-icon-page">
      <path d="M5 3h22v27l-4-3-4 3-4-3-4 3-6-3Z" fill={paper} /><path d="M10 8h12m-12 5h12m-12 5h7m-7 5h12" stroke={ink} />
    </g>,
    profiles: <g className="theme-icon-rock">
      <path d="M3 11h12v18H3Zm14-8h12v26H17Z" fill={paper} /><path d="M3 17h12m2-7h12" stroke={color} strokeWidth="3" /><circle cx="9" cy="23" r="2" fill={shade} stroke="none" /><path d="M20 16h6v8h-6Z" fill={shade} />
    </g>,
    settings: <g className="theme-icon-rock">
      <path d="M12 3h8l-1 10 7 12q2 5-4 5H10q-6 0-4-5l7-12Z" fill={color} /><path d="M12 3h8v7h-8Z" fill={shade} /><path d="M7 23h18v5H7Z" fill={paper} /><path d="M15 17v3" stroke={paper} />
    </g>,
  },
};
