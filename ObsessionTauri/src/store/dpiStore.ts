import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, on, type AppConfig, type ConfStat, type DpiProc } from "../lib/tauri";

const CATEGORY_ORDER = ["discord", "youtube_twitch", "gaming", "universal"];

interface DpiState {
  config: AppConfig | null;
  selectedCategories: string[];
  selectedConfigs: Record<string, string>;
  active: boolean;
  processes: DpiProc[];
  transitioning: boolean;
  testing: boolean;
  testCancel: boolean;
  testingLabel: string;
  testResults: Record<string, boolean>;
  netStats: Record<string, ConfStat>;
  error: string;

  bootstrap: () => Promise<UnlistenFn>;
  loadStats: () => Promise<void>;
  toggleCategory: (cat: string) => void;
  setConfig: (cat: string, file: string) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  testAll: () => Promise<void>;
  cancelTest: () => void;
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
  testCancel: false,
  testingLabel: "",
  testResults: {},
  netStats: {},
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
    // Возвращаем UnlistenFn наверх (App) — иначе слушатель жил бы вечно и
    // дублировался при повторном bootstrap (StrictMode).
    const unlisten = await on.dpiStatus((s) => set({ active: s.active, processes: s.processes }));
    await get().loadStats();
    return unlisten;
  },

  loadStats: async () => {
    try {
      const stats = await api.getNetcacheStats();
      set({ netStats: stats });
    } catch {
      /* offline/нет сети — ничего */
    }
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

  // Проверяет выбранный конфиг КАЖДОЙ выбранной категории (по одному тесту на
  // категорию — быстро и покрывает весь выбор). Полный перебор конфигов делает
  // autoConfigure. Прерывается флагом testCancel между категориями.
  testAll: async () => {
    const { selectedCategories, selectedConfigs, config } = get();
    set({ testing: true, testCancel: false, testResults: {} });
    const results: Record<string, boolean> = {};
    for (const cat of selectedCategories) {
      if (get().testCancel) break;
      const files = config?.configs[cat] ?? [];
      const file = selectedConfigs[cat] || defaultConfig(files);
      if (!file) continue;
      set({ testingLabel: `${cat}: ${file}` });
      const ok = await api.dpiTest(cat, file);
      results[file] = ok;
      if (ok) await api.recordWorkingConfig(cat, file);
      set({ testResults: { ...results } });
    }
    set({ testing: false, testingLabel: "", testCancel: false });
    await get().loadStats(); // Обновить статистику надёжности
  },

  // Отмена теста: помечаем флаг (цикл прервётся между категориями) и просим
  // бэкенд оборвать текущий тест — обход тут же освобождается.
  cancelTest: () => {
    if (!get().testing) return;
    set({ testCancel: true, testingLabel: "Отмена…" });
    api.dpiTestCancel().catch(() => {});
  },

  autoConfigure: async () => {
    const { selectedCategories, config } = get();
    set({ testing: true, testCancel: false, testResults: {} });
    const selected = { ...get().selectedConfigs };
    outer: for (const cat of selectedCategories) {
      const files = config?.configs[cat] ?? [];
      set({ testingLabel: cat });
      for (const file of files) {
        if (get().testCancel) break outer;
        set({ testingLabel: `${cat}: ${file}` });
        const ok = await api.dpiTest(cat, file);
        if (ok) {
          await api.recordWorkingConfig(cat, file);
          selected[cat] = file;
          break;
        }
      }
    }
    set({ testing: false, testingLabel: "", testCancel: false, selectedConfigs: selected });
    persist();
    await get().loadStats(); // Обновить статистику надёжности
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
