import { create } from "zustand";
import { api, on, type AppConfig, type DpiProc } from "../lib/tauri";

const CATEGORY_ORDER = ["discord", "youtube_twitch", "gaming", "universal"];

interface DpiState {
  config: AppConfig | null;
  selectedCategories: string[];
  selectedConfigs: Record<string, string>;
  active: boolean;
  processes: DpiProc[];
  transitioning: boolean;
  testing: boolean;
  testingLabel: string;
  testResults: Record<string, boolean>;
  error: string;

  bootstrap: () => Promise<void>;
  toggleCategory: (cat: string) => void;
  setConfig: (cat: string, file: string) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  testAll: (cat: string) => Promise<void>;
  autoConfigure: () => Promise<void>;
  clearError: () => void;
}

/// Дефолтный конфиг категории: <cat>_1.conf или первый доступный.
function defaultConfig(files: string[]): string {
  return files.find((f) => f.includes("_1.conf")) ?? files[0] ?? "";
}

export const useDpiStore = create<DpiState>((set, get) => ({
  config: null,
  selectedCategories: ["discord"],
  selectedConfigs: {},
  active: false,
  processes: [],
  transitioning: false,
  testing: false,
  testingLabel: "",
  testResults: {},
  error: "",

  bootstrap: async () => {
    const config = await api.getConfig();
    const settings = await api.getSettings();
    const selectedConfigs: Record<string, string> = {};
    for (const cat of CATEGORY_ORDER) {
      const files = config.configs[cat] ?? [];
      if (files.length === 0) continue;
      const stored = settings.selected_configs[cat];
      selectedConfigs[cat] =
        stored && files.includes(stored) ? stored : defaultConfig(files);
    }
    const selectedCategories =
      settings.selected_categories.length > 0
        ? settings.selected_categories.filter((c) => config.categories.includes(c))
        : ["discord"];
    set({
      config,
      selectedConfigs,
      selectedCategories: selectedCategories.length ? selectedCategories : ["discord"],
    });

    // Подписка на статус DPI от бэкенда. НЕ трогает transitioning — им владеют
    // start()/stop() (сбрасывают в finally). Иначе первый же dpi-status снимал
    // блокировку кнопки до конца операции → повторный клик ловил гонку.
    on.dpiStatus((s) => set({ active: s.active, processes: s.processes }));
  },

  toggleCategory: (cat) => {
    if (get().active) return;
    const sel = new Set(get().selectedCategories);
    if (sel.has(cat)) {
      if (sel.size > 1) sel.delete(cat);
    } else {
      sel.add(cat);
    }
    set({ selectedCategories: [...sel] });
    persist();
  },

  setConfig: (cat, file) => {
    if (get().active) return;
    set({ selectedConfigs: { ...get().selectedConfigs, [cat]: file } });
    persist();
  },

  start: async () => {
    const { selectedCategories, selectedConfigs, transitioning } = get();
    if (transitioning) return;
    set({ transitioning: true, error: "" });
    const configs = selectedCategories.map((category) => ({
      category,
      config_file: selectedConfigs[category] ?? "",
    }));
    try {
      await api.dpiStart(configs);
      // active/processes придут подпиской dpi-status ещё до resolve.
    } catch (e) {
      set({ active: false, error: String(e) });
    } finally {
      // Держим блокировку до конца операции (включая старт Глаз на бэкенде).
      set({ transitioning: false });
    }
  },

  stop: async () => {
    if (get().transitioning) return;
    set({ transitioning: true });
    try {
      await api.dpiStop();
    } finally {
      set({ transitioning: false });
    }
  },

  testAll: async (cat) => {
    const files = get().config?.configs[cat] ?? [];
    set({ testing: true, testResults: {}, testingLabel: cat });
    const results: Record<string, boolean> = {};
    for (const file of files) {
      set({ testingLabel: `${cat}: ${file}` });
      const ok = await api.dpiTest(cat, file);
      results[file] = ok;
      set({ testResults: { ...results } });
    }
    set({ testing: false, testingLabel: "" });
  },

  autoConfigure: async () => {
    const { selectedCategories, config } = get();
    set({ testing: true, testResults: {} });
    const selected = { ...get().selectedConfigs };
    for (const cat of selectedCategories) {
      const files = config?.configs[cat] ?? [];
      set({ testingLabel: cat });
      for (const file of files) {
        set({ testingLabel: `${cat}: ${file}` });
        const ok = await api.dpiTest(cat, file);
        if (ok) {
          selected[cat] = file;
          break;
        }
      }
    }
    set({ testing: false, testingLabel: "", selectedConfigs: selected });
    persist();
  },

  clearError: () => set({ error: "" }),
}));

/// Сохраняет выбор категорий/конфигов в настройки бэкенда.
async function persist() {
  const { selectedCategories, selectedConfigs } = useDpiStore.getState();
  try {
    const settings = await api.getSettings();
    await api.saveSettings({
      ...settings,
      selected_categories: selectedCategories,
      selected_configs: selectedConfigs,
    });
  } catch {
    /* best-effort */
  }
}
