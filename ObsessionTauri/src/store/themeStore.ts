import { create } from "zustand";

// Визуальная тема hero-элемента и фона. Чисто фронтовая настройка (не трогает
// Rust-Settings): храним в localStorage, применяем мгновенно.
export type Theme =
  | "goldenmeadow"
  | "obsession"
  | "aurora"
  | "ophanim"
  | "japan"
  | "midnight"
  | "catnap"
  | "fallendown"
  | "yanineko";

const THEME_IDS: Theme[] = [
  "goldenmeadow",
  "obsession",
  "aurora",
  "ophanim",
  "japan",
  "midnight",
  "catnap",
  "fallendown",
  "yanineko",
];

// `secret` — id пасхалки в secretStore; такая тема появляется в выборе только
// после разблокировки (см. окошко пасхалок в Настройках).
export const THEMES: { id: Theme; label: string; secret?: string }[] = [
  { id: "goldenmeadow", label: "Golden Meadow" },
  { id: "obsession", label: "Obsession" },
  { id: "aurora", label: "Aurora" },
  // Keep the persisted slot: existing Ophanim selections become Alchemist.
  { id: "ophanim", label: "Alchemist" },
  { id: "japan", label: "Rain" },
  { id: "midnight", label: "Midnight" },
  { id: "catnap", label: "Catnap", secret: "catnap" },
  { id: "fallendown", label: "Fallen Down", secret: "fallendown" },
  { id: "yanineko", label: "Yani Neko", secret: "yanineko" },
];

const KEY = "obsession.theme";

export function resolveStoredTheme(value: string | null): Theme {
  // Existing QuietPond selections follow the theme slot that replaced it.
  if (value === "quietpond") return "goldenmeadow";
  return value && THEME_IDS.includes(value as Theme)
    ? (value as Theme)
    : "obsession";
}

function load(): Theme {
  try {
    return resolveStoredTheme(localStorage.getItem(KEY));
  } catch {
    /* localStorage может быть недоступен — молча откатываемся к дефолту */
  }
  return "obsession";
}

interface ThemeState {
  theme: Theme;
  setTheme: (t: Theme) => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: load(),
  setTheme: (theme) => {
    try {
      localStorage.setItem(KEY, theme);
    } catch {
      /* игнорируем ошибки записи */
    }
    set({ theme });
  },
}));
