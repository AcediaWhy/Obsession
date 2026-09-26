import { create } from "zustand";

// Визуальная тема hero-элемента и фона. Чисто фронтовая настройка (не трогает
// Rust-Settings): храним в localStorage, применяем мгновенно.
export type Theme =
  | "goldenmeadow"
  | "aurora"
  | "ophanim"
  | "japan"
  | "midnight"
  | "catnap"
  | "fallendown"
  | "yanineko";

const THEME_IDS: Theme[] = [
  "goldenmeadow",
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
  { id: "aurora", label: "Aurora" },
  // Сохраняем прежний идентификатор: выбор Ophanim теперь открывает Alchemist.
  { id: "ophanim", label: "Alchemist" },
  { id: "japan", label: "Rain" },
  { id: "midnight", label: "Midnight" },
  { id: "catnap", label: "Catnap", secret: "catnap" },
  { id: "fallendown", label: "Fallen Down", secret: "fallendown" },
  { id: "yanineko", label: "Yani Neko", secret: "yanineko" },
];

const KEY = "obsession.theme";
const DEFAULT_THEME: Theme = "goldenmeadow";

export function resolveStoredTheme(value: string | null): Theme {
  // Удалённые темы переводятся на доступную тему до создания UI.
  if (value === "quietpond" || value === "obsession") return DEFAULT_THEME;
  return value && THEME_IDS.includes(value as Theme)
    ? (value as Theme)
    : DEFAULT_THEME;
}

function load(): Theme {
  try {
    const stored = localStorage.getItem(KEY);
    const resolved = resolveStoredTheme(stored);
    if (stored !== resolved) localStorage.setItem(KEY, resolved);
    return resolved;
  } catch {
    /* localStorage может быть недоступен — молча откатываемся к дефолту */
  }
  return DEFAULT_THEME;
}

interface ThemeState {
  theme: Theme;
  setTheme: (t: Theme) => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: load(),
  setTheme: (theme) => {
    const resolved = resolveStoredTheme(theme);
    try {
      localStorage.setItem(KEY, resolved);
    } catch {
      /* игнорируем ошибки записи */
    }
    set({ theme: resolved });
  },
}));
