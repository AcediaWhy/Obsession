import { create } from "zustand";
import { api, type Settings } from "../lib/tauri";
import { toast } from "./toastStore";

interface SettingsState {
  loaded: boolean;
  saving: boolean;
  saved: boolean; // короткий флаг «сохранено» для галочки
  settings: Settings | null;
  elevated: boolean;
  autostart: boolean; // источник истины — реестр Windows, не settings.json
  error: string;

  bootstrap: () => Promise<void>;
  patch: (partial: Partial<Settings>) => Promise<void>;
  setAutostart: (enable: boolean) => Promise<void>;
}

// Раздел настроек поверх реального Settings-payload'а из Rust. Любое изменение
// сразу персистится (best-effort) — без отдельной кнопки «Сохранить».
export const useSettingsStore = create<SettingsState>((set, get) => ({
  loaded: false,
  saving: false,
  saved: false,
  settings: null,
  elevated: false,
  autostart: false,
  error: "",

  bootstrap: async () => {
    try {
      const [settings, elevated, autostart] = await Promise.all([
        api.getSettings(),
        api.isElevated().catch(() => false),
        api.getAutostart().catch(() => false),
      ]);
      set({ settings, elevated, autostart, loaded: true });
    } catch (e) {
      set({ error: String(e), loaded: true });
    }
  },

  patch: async (partial) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...partial };
    // Оптимистично применяем в UI, затем персистим.
    set({ settings: next, saving: true, error: "" });
    try {
      await api.saveSettings(next);
      set({ saving: false, saved: true });
      setTimeout(() => set({ saved: false }), 1600);
    } catch (e) {
      set({ saving: false, error: String(e) });
      toast.error("Не удалось сохранить настройки");
    }
  },

  setAutostart: async (enable) => {
    // Оптимистично применяем; при ошибке — откатываем.
    set({ autostart: enable, error: "" });
    try {
      await api.setAutostart(enable);
    } catch (e) {
      set({ autostart: !enable, error: String(e) });
      toast.error(String(e));
    }
  },
}));
