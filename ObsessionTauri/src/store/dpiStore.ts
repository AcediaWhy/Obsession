import { create } from "zustand";
import {
  api,
  runtime,
  type AppConfig,
  type ConfStat,
  type DpiProc,
  type DpiStatus,
  type DpiTestReport,
  type EngineOption,
  type Settings,
  type VersionedSection,
  type Zapret2ProfileDescriptor,
} from "../lib/tauri";
import { withDeadline } from "../lib/asyncDeadline";

const CATEGORY_ORDER = ["discord", "youtube_twitch", "gaming", "universal"];
const ZAPRET2_CATEGORIES = new Set(["discord", "youtube_twitch", "gaming"]);
export const TRANSITION_WATCHDOG_MS = 8_000;
export const TRANSITION_RECONCILE_TIMEOUT_MS = 4_000;

type CategorySelections = {
  legacy: string[];
  zapret2: string[];
};

interface DpiState {
  revision: number;
  config: AppConfig | null;
  selectedCategories: string[];
  categorySelections: CategorySelections;
  selectedConfigs: Record<string, string>;
  active: boolean;
  processes: DpiProc[];
  startedAt: number | null;
  transitioning: boolean;
  testing: boolean;
  testCancel: boolean;
  testingLabel: string;
  testResults: Record<string, boolean>;
  testReports: Record<string, DpiTestReport>;
  netStats: Record<string, ConfStat>;
  error: string;
  engines: EngineOption[];
  zapret2Profiles: Zapret2ProfileDescriptor[];

  initialize: (config: AppConfig, settings: Settings) => void;
  applyVersionedStatus: (section: VersionedSection<DpiStatus>) => boolean;
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

function statusPatch(status: DpiStatus) {
  return {
    active: status.active,
    processes: status.processes,
    startedAt: status.started_at != null ? status.started_at * 1000 : null,
  };
}

export const useDpiStore = create<DpiState>((set, get) => ({
  revision: -1,
  config: null,
  selectedCategories: ["discord"],
  categorySelections: { legacy: ["discord"], zapret2: ["discord"] },
  selectedConfigs: {},
  active: false,
  processes: [],
  startedAt: null,
  transitioning: false,
  testing: false,
  testCancel: false,
  testingLabel: "",
  testResults: {},
  testReports: {},
  netStats: {},
  error: "",
  engines: [],
  zapret2Profiles: [],

  initialize: (config, settings) => {
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
        ? settings.selected_categories.filter((category) =>
            CATEGORY_ORDER.includes(category) && config.categories.includes(category),
          )
        : ["discord"];
    const zapret2 =
      settings.zapret2_selected_categories.length > 0
        ? settings.zapret2_selected_categories.filter((category) =>
            ZAPRET2_CATEGORIES.has(category),
          )
        : ["discord"];
    const categorySelections: CategorySelections = {
      legacy: legacy.length ? legacy : ["discord"],
      zapret2: zapret2.length ? zapret2 : ["discord"],
    };
    set({
      config,
      selectedConfigs,
      categorySelections,
      selectedCategories:
        settings.dpi_engine === "zapret2"
          ? categorySelections.zapret2
          : categorySelections.legacy,
      transitioning: false,
      error: "",
    });
  },

  // Применяет живой dpi-status. НЕ трогает transitioning: ранний status во время
  // штатного start/stop не должен снимать защиту от повторного клика. startedAt —
  // из backend (Unix-сек → мс), источник аптайма, переживающий смену вкладок/resume.
  applyVersionedStatus: (section) => {
    if (section.revision <= get().revision) return false;
    set({ ...statusPatch(section.value), revision: section.revision });
    return true;
  },

  applyStatus: (status) => set(statusPatch(status)),

  // Аварийная сверка frontend latch с точным backend runtime. Вызывается только
  // watchdog-ом после нормального окна start/stop, поэтому не конкурирует с
  // обычными быстрыми переходами.
  reconcileTransition: async () => {
    if (!get().transitioning) return;
    try {
      const snapshot = await withDeadline(
        runtime.bootstrap(),
        TRANSITION_RECONCILE_TIMEOUT_MS,
        "Backend не ответил на сверку состояния DPI.",
      );
      if (
        !get().transitioning ||
        snapshot.dpi.revision < get().revision
      ) {
        return;
      }
      set({
        ...statusPatch(snapshot.dpi.value),
        revision: Math.max(get().revision, snapshot.dpi.revision),
        transitioning: false,
      });
    } catch (error) {
      // Backend status остаётся источником истины и может прийти позднее.
      // Но frontend latch нельзя оставлять навсегда: пользователь должен иметь
      // возможность повторить действие без перезапуска приложения.
      set({ transitioning: false, error: String(error) });
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
    if (get().active || get().transitioning || get().testing) return;
    set({ transitioning: true, error: "" });
    try {
      await withDeadline(api.dpiEngineSet(kind), TRANSITION_WATCHDOG_MS,
        "Смена движка не ответила вовремя. Проверяем состояние службы.");
      const selectedCategories =
        kind === "zapret2"
          ? get().categorySelections.zapret2
          : get().categorySelections.legacy;
      set({ selectedCategories });
      await Promise.all([get().loadEngines(), get().loadZapret2Profiles()]);
    } catch (e) {
      set({ error: String(e) });
    } finally {
      await refreshDpiAfterOperation();
      set({ transitioning: false });
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
      await withDeadline(
        api.dpiStart(configs),
        TRANSITION_WATCHDOG_MS,
        "Запуск DPI не ответил вовремя. Состояние будет синхронизировано автоматически.",
      );
      // active/processes придут подпиской dpi-status ещё до resolve.
    } catch (e) {
      // Не перетираем active: service мог успеть запустить runtime, а ответ
      // потеряться/опоздать. Versioned status или следующий snapshot уточнит итог.
      set({ error: String(e) });
    } finally {
      // Also reconcile rejected/timed-out commands: the service can have
      // completed them even when their response/event never reached the UI.
      await refreshDpiAfterOperation();
      set({ transitioning: false });
    }
  },

  stop: async () => {
    if (get().transitioning) return;
    set({ transitioning: true });
    try {
      await withDeadline(
        api.dpiStop(),
        TRANSITION_WATCHDOG_MS,
        "Остановка DPI не ответила вовремя. Состояние будет синхронизировано автоматически.",
      );
    } catch (e) {
      set({ error: String(e) });
    } finally {
      await refreshDpiAfterOperation();
      set({ transitioning: false });
    }
  },

  // Проверяет выбранный конфиг КАЖДОЙ выбранной категории (по одному тесту на
  // категорию — быстро и покрывает весь выбор). Полный перебор конфигов делает
  // autoConfigure. Прерывается флагом testCancel между категориями.
  testAll: async () => {
    if (get().testing || get().transitioning || get().active) return;
    const { selectedCategories, selectedConfigs, config } = get();
    set({ testing: true, testCancel: false, testResults: {}, testReports: {}, error: "" });
    const results: Record<string, boolean> = {};
    try {
      for (const cat of selectedCategories) {
        if (get().testCancel) break;
        const files = config?.configs[cat] ?? [];
        const file = selectedConfigs[cat] || defaultConfig(files);
        if (!file) continue;
        set({ testingLabel: `${cat}: ${file}` });
        const report = await api.dpiTest(cat, file);
        if (get().testCancel || report.status === "cancelled") break;
        const ok = report.passed;
        set((state) => ({ testReports: { ...state.testReports, [file]: report } }));
        results[file] = ok;
        if (ok) await api.recordWorkingConfig(cat, file);
        set({ testResults: { ...results } });
      }
    } catch (e) {
      // Реджект dpi_test/record_working_config НЕ должен оставить testing=true
      // навсегда (кнопки Тест/Авто-подбор залипли бы на «Отменить» до рестарта).
      set({ error: String(e) });
    } finally {
      await refreshDpiAfterOperation();
      set({ testing: false, testingLabel: "", testCancel: false });
    }
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
    if (get().testing || get().transitioning || get().active) return;
    const { selectedCategories, config } = get();
    set({ testing: true, testCancel: false, testResults: {}, testReports: {}, error: "" });
    const selected = { ...get().selectedConfigs };
    const failed: string[] = [];
    try {
      outer: for (const cat of selectedCategories) {
        const available = config?.configs[cat] ?? [];
        // Сначала перепроверяем успешный для этой сети и текущий варианты.
        // Кэш определяет порядок, но не заменяет сетевую проверку.
        const files = [...new Set([
          get().netStats[cat]?.conf,
          selected[cat],
          ...available,
        ])].filter((file): file is string => !!file && available.includes(file));
        let found = false;
        set({ testingLabel: cat });
        for (const [index, file] of files.entries()) {
          if (get().testCancel) break outer;
          set({ testingLabel: `${cat}: ${file} (${index + 1}/${files.length})` });
          const report = await api.dpiTest(cat, file, true);
          if (get().testCancel || report.status === "cancelled") break outer;
          const ok = report.passed;
          set((state) => ({ testReports: { ...state.testReports, [file]: report } }));
          set((state) => ({ testResults: { ...state.testResults, [file]: ok } }));
          if (ok) {
            await api.recordWorkingConfig(cat, file);
            selected[cat] = file;
            found = true;
            break;
          }
        }
        if (!found) failed.push(cat);
      }
      if (failed.length) set({ error: `Не найден конфиг, прошедший все проверки: ${failed.join(", ")}. Предыдущий выбор сохранён. Частичные результаты доступны в подробностях проверки.` });
    } catch (e) {
      // Как в testAll: любой реджект внутри цикла обязан снять testing-латч.
      set({ error: String(e) });
    } finally {
      await refreshDpiAfterOperation();
      set({ testing: false, testingLabel: "", testCancel: false, selectedConfigs: selected });
    }
    persist();
    await get().loadStats(); // Обновить статистику надёжности
  },

  clearError: () => set({ error: "" }),
}));

async function refreshDpiAfterOperation() {
  try {
    const snapshot = await withDeadline(runtime.bootstrap(), TRANSITION_RECONCILE_TIMEOUT_MS,
      "Не удалось сверить состояние DPI со службой.");
    const current = useDpiStore.getState();
    // A snapshot may legitimately share the last event's revision. Only a
    // strictly older observation is discarded; do not lose late live events.
    if (snapshot.dpi.revision >= current.revision) {
      useDpiStore.setState({ ...statusPatch(snapshot.dpi.value), revision: snapshot.dpi.revision });
    }
  } catch {
    // Preserve the last known status and the original operation error.
  }
}

function currentEngine(engines: EngineOption[]): keyof CategorySelections {
  return engines.some((engine) => engine.kind === "zapret2" && engine.selected)
    ? "zapret2"
    : "legacy";
}

/// Сохраняет выбор категорий/конфигов в настройки бэкенда.
///
/// Вызовы сериализуются через `dpiWriteQueue`: `toggleCategory`/`setConfig`
/// дёргают persist() как fire-and-forget, а Tauri-команды выполняются на Rust
/// конкурентно — без очереди старый (полный) payload мог завершиться ПОСЛЕ
/// нового и восстановить устаревший выбор при следующем запуске. Каждое звено
/// заново читает актуальный state, поэтому последний вызов персистит свежий снимок.
let dpiWriteQueue: Promise<unknown> = Promise.resolve();

async function persist() {
  const run = dpiWriteQueue.then(async () => {
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
  });
  // Хвост очереди не должен «застрять» на реджекте (свести к резолву).
  dpiWriteQueue = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}
