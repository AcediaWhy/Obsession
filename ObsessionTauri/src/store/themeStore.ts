import { create } from "zustand";

// Визуальная тема hero-элемента и фона. Чисто фронтовая настройка (не трогает
// Rust-Settings): храним в localStorage, применяем мгновенно.
export type Theme = "aurora" | "ophanim" | "japan" | "fireflies" | "hearth" | "fallendown";

const THEME_IDS: Theme[] = ["aurora", "ophanim", "japan", "fireflies", "hearth", "fallendown"];

// `secret` — id пасхалки в secretStore; такая тема появляется в выборе только
// после разблокировки (см. окошко пасхалок в Настройках).
export const THEMES: { id: Theme; label: string; secret?: string }[] = [
  { id: "aurora", label: "Aurora" },
  { id: "ophanim", label: "Ophanim" },
  { id: "japan", label: "Rain" },
  { id: "fireflies", label: "Fireflies" },
  { id: "hearth", label: "Hearth" },
  { id: "fallendown", label: "Fallen Down", secret: "fallendown" },
];

const KEY = "obsession.theme";

function load(): Theme {
  try {
    const v = localStorage.getItem(KEY) as Theme | null;
    if (v && THEME_IDS.includes(v)) return v;
  } catch {
    /* localStorage может быть недоступен — молча откатываемся к дефолту */
  }
  return "aurora";
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

