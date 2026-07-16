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

// Последовательная очередь сохраняет пользовательский порядок быстрых patch-вызовов.
let settingsWriteQueue: Promise<unknown> = Promise.resolve();
let settingsPatchSequence = 0;

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
    const sequence = ++settingsPatchSequence;
    // Оптимистично применяем только изменённые поля. Backend объединит patch с
    // актуальным Settings под lock, поэтому другие stores не будут затёрты.
    set({ settings: { ...current, ...partial }, saving: true, error: "" });
    const request = settingsWriteQueue.then(() => api.updateSettings(partial));
    settingsWriteQueue = request.then(
      () => undefined,
      () => undefined,
    );
    try {
      const savedSettings = await request;
      // Старый response не должен перезаписать более свежий optimistic patch.
      if (sequence === settingsPatchSequence) {
        set({ settings: savedSettings, saving: false, saved: true });
        setTimeout(() => {
          if (sequence === settingsPatchSequence) set({ saved: false });
        }, 1600);
      }
    } catch (e) {
      if (sequence === settingsPatchSequence) {
        const authoritative = await api.getSettings().catch(() => get().settings);
        set({ settings: authoritative, saving: false, error: String(e) });
      }
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
