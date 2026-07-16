import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  on,
  runtime,
  type AppConfig,
  type ConfStat,
  type DpiProc,
  type DpiStatus,
  type EngineOption,
  type Zapret2ProfileDescriptor,
} from "../lib/tauri";

const CATEGORY_ORDER = ["discord", "youtube_twitch", "gaming", "universal", "atrisk"];
const ZAPRET2_CATEGORIES = new Set(["discord", "youtube_twitch", "gaming"]);
export const TRANSITION_WATCHDOG_MS = 8_000;

type CategorySelections = {
  legacy: string[];
  zapret2: string[];
};

interface DpiState {
  config: AppConfig | null;
  selectedCategories: string[];
  categorySelections: CategorySelections;
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
  engines: EngineOption[];
  zapret2Profiles: Zapret2ProfileDescriptor[];

  bootstrap: () => Promise<UnlistenFn>;
  applyStatus: (s: DpiStatus) => void;
  reconcileTransition: () => Promise<void>;
  loadStats: () => Promise<void>;
  loadEngines: () => Promise<void>;
  loadZapret2Profiles: () => Promise<void>;
  setEngine: (kind: string) => Promise<void>;
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
  categorySelections: { legacy: ["discord"], zapret2: ["discord"] },
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
  engines: [],
  zapret2Profiles: [],

  bootstrap: async () => {
    const [config, settings, snapshot] = await Promise.all([
      api.getConfig(),
      api.getSettings(),
      runtime.snapshot().catch(() => null),
    ]);
    const selectedConfigs: Record<string, string> = {};
    for (const cat of CATEGORY_ORDER) {
      const files = config.configs[cat] ?? [];
      if (files.length === 0) continue;
      const stored = settings.selected_configs[cat];
      selectedConfigs[cat] =
        stored && files.includes(stored) ? stored : defaultConfig(files);
    }
    const legacy =
      settings.selected_categories.length > 0
        ? settings.selected_categories.filter((c) => config.categories.includes(c))
        : ["discord"];
    const zapret2 =
      settings.zapret2_selected_categories.length > 0
        ? settings.zapret2_selected_categories.filter((c) => ZAPRET2_CATEGORIES.has(c))
        : ["discord"];
    const categorySelections: CategorySelections = {
      legacy: legacy.length ? legacy : ["discord"],
      zapret2: zapret2.length ? zapret2 : ["discord"],
    };
    const selectedCategories =
      settings.dpi_engine === "zapret2"
        ? categorySelections.zapret2
        : categorySelections.legacy;
    set({
      config,
      selectedConfigs,
      selectedCategories,
      categorySelections,
      active: snapshot?.dpi.active ?? get().active,
      processes: snapshot?.dpi.processes ?? get().processes,
      // Bootstrap означает новый frontend lifecycle: незавершённого локального
      // start/stop promise здесь уже нет, поэтому backend snapshot авторитетен.
      transitioning: false,
    });

    // Подписка на статус DPI от бэкенда. НЕ трогает transitioning — им владеют
    // start()/stop() (сбрасывают в finally). Иначе первый же dpi-status снимал
    // блокировку кнопки до конца операции → повторный клик ловил гонку.
    // Возвращаем UnlistenFn наверх (App) — иначе слушатель жил бы вечно и
    // дублировался при повторном bootstrap (StrictMode).
    const unlisten = await on.dpiStatus((s) => {
      get().applyStatus(s);
      // Crash fallback меняет выбранный engine на backend. Обновляем chips после
      // status event, чтобы UI не продолжал показывать Zapret2 при активном Legacy.
      void Promise.all([get().loadEngines(), get().loadZapret2Profiles()]);
    });
    await Promise.all([
      get().loadStats(),
      get().loadEngines(),
      get().loadZapret2Profiles(),
    ]);
    return unlisten;
  },

  // Применяет живой dpi-status. НЕ трогает transitioning: ранний status во время
  // штатного start/stop не должен снимать защиту от повторного клика.
  applyStatus: (s) => set({ active: s.active, processes: s.processes }),

  // Аварийная сверка frontend latch с точным backend runtime. Вызывается только
  // watchdog-ом после нормального окна start/stop, поэтому не конкурирует с
  // обычными быстрыми переходами.
  reconcileTransition: async () => {
    if (!get().transitioning) return;
    const epoch = runtime.statusEpoch();
    try {
      const snapshot = await runtime.snapshot();
      if (!get().transitioning || runtime.statusEpoch() !== epoch) return;
      set({
        active: snapshot.dpi.active,
        processes: snapshot.dpi.processes,
        transitioning: false,
      });
    } catch {
      // Без подтверждённого snapshot сохраняем блокировку: догадки опаснее
      // краткого busy-состояния.
    }
  },

  loadStats: async () => {
    try {
      const stats = await api.getNetcacheStats();
      set({ netStats: stats });
    } catch {
      /* offline/нет сети — ничего */
    }
  },

  loadEngines: async () => {
    try {
      const engines = await api.dpiEngineList();
      const engine = currentEngine(engines);
      set({ engines, selectedCategories: get().categorySelections[engine] });
    } catch {
      /* движки не критичны для основного потока */
    }
  },

  loadZapret2Profiles: async () => {
    try {
      const categories = get().categorySelections.zapret2;
      set({ zapret2Profiles: await api.dpiZapret2Profiles(categories) });
    } catch {
      set({ zapret2Profiles: [] });
    }
  },

  setEngine: async (kind) => {
    if (get().active) return; // не меняем движок при активном обходе
    try {
      await api.dpiEngineSet(kind);
      const selectedCategories =
        kind === "zapret2"
          ? get().categorySelections.zapret2
          : get().categorySelections.legacy;
      set({ selectedCategories });
      await Promise.all([get().loadEngines(), get().loadZapret2Profiles()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  toggleCategory: (cat) => {
    if (get().active) return;
    const engine = currentEngine(get().engines);
    if (engine === "zapret2" && !ZAPRET2_CATEGORIES.has(cat)) return;
    const sel = new Set(get().selectedCategories);
    if (sel.has(cat)) {
      if (sel.size > 1) sel.delete(cat);
    } else {
      sel.add(cat);
    }
    const selectedCategories = [...sel];
    set({
      selectedCategories,
      categorySelections: {
        ...get().categorySelections,
        [engine]: selectedCategories,
      },
    });
    persist();
    void get().loadZapret2Profiles();
  },

  setConfig: (cat, file) => {
    if (get().active) return;
    set({ selectedConfigs: { ...get().selectedConfigs, [cat]: file } });
    persist();
  },

  start: async () => {
    const { selectedConfigs, transitioning } = get();
    if (transitioning) return;
    set({ transitioning: true, error: "" });
    const engine = currentEngine(get().engines);
    const selectedCategories =
      engine === "zapret2"
        ? get().selectedCategories.filter((category) => ZAPRET2_CATEGORIES.has(category))
        : get().selectedCategories;
    set({ selectedCategories });
    const configs = selectedCategories.map((category) => ({
      category,
      config_file: selectedConfigs[category] ?? "",
    }));
    try {
      await persist();
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

function currentEngine(engines: EngineOption[]): keyof CategorySelections {
  return engines.some((engine) => engine.kind === "zapret2" && engine.selected)
    ? "zapret2"
    : "legacy";
}

/// Сохраняет выбор категорий/конфигов в настройки бэкенда.
async function persist() {
  const state = useDpiStore.getState();
  const engine = currentEngine(state.engines);
  const categorySelections = {
    ...state.categorySelections,
    [engine]: state.selectedCategories,
  };
  useDpiStore.setState({ categorySelections });
  try {
    await api.updateSettings({
      selected_categories: categorySelections.legacy,
      zapret2_selected_categories: categorySelections.zapret2,
      selected_configs: state.selectedConfigs,
    });
  } catch {
    /* best-effort */
  }
}
