import { create } from "zustand";
import { api, type Profile } from "../lib/tauri";
import { useDpiStore } from "./dpiStore";
import { useProxyStore } from "./proxyStore";

interface ProfileState {
  profiles: Profile[];
  loaded: boolean;
  busy: boolean; // идёт сохранение/удаление
  applyingId: string; // id профиля, который сейчас применяется ("" — нет)
  error: string;

  bootstrap: () => Promise<void>;
  saveCurrent: (name: string) => Promise<void>;
  update: (profile: Profile) => Promise<void>;
  remove: (id: string) => Promise<void>;
  apply: (profile: Profile) => Promise<void>;
  clearError: () => void;
}

// Простой генератор id (crypto.randomUUID доступен в WebView2).
function newId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `p_${Date.now()}_${Math.floor(Math.random() * 1e6)}`;
  }
}

export const useProfileStore = create<ProfileState>((set) => ({
  profiles: [],
  loaded: false,
  busy: false,
  applyingId: "",
  error: "",

  bootstrap: async () => {
    try {
      const profiles = await api.getProfiles();
      set({ profiles, loaded: true });
    } catch (e) {
      set({ error: String(e), loaded: true });
    }
  },

  // Снимок текущего состояния приложения → новый профиль.
  saveCurrent: async (name) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    set({ busy: true, error: "" });
    const dpi = useDpiStore.getState();
    const proxy = useProxyStore.getState();
    let ai_provider = "malw";
    try {
      ai_provider = (await api.getSettings()).ai_provider || "malw";
    } catch {
      /* best-effort */
    }
    const profile: Profile = {
      id: newId(),
      name: trimmed,
      selected_categories: [...dpi.selectedCategories],
      selected_configs: { ...dpi.selectedConfigs },
      proxy_port: proxy.port,
      fake_tls_domain: proxy.fakeTlsDomain,
      ai_provider,
    };
    try {
      const profiles = await api.saveProfile(profile);
      set({ profiles, busy: false });
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  // Перезапись существующего профиля (напр. после переименования).
  update: async (profile) => {
    set({ busy: true, error: "" });
    try {
      const profiles = await api.saveProfile(profile);
      set({ profiles, busy: false });
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  remove: async (id) => {
    set({ busy: true, error: "" });
    try {
      const profiles = await api.deleteProfile(id);
      set({ profiles, busy: false });
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  // Применяет пресет: конфигурирует сторы, персистит и запускает DPI + прокси.
  apply: async (profile) => {
    set({ applyingId: profile.id, error: "" });

    // 1. Персистим ИИ-провайдера в настройки (без авто-установки hosts).
    try {
      await api.updateSettings({ ai_provider: profile.ai_provider });
    } catch {
      /* best-effort */
    }

    // 2. DPI: задаём выбор и запускаем (бэкенд сам гасит предыдущие процессы).
    useDpiStore.setState({
      selectedCategories: profile.selected_categories.length
        ? [...profile.selected_categories]
        : ["discord"],
      selectedConfigs: { ...profile.selected_configs },
    });
    try {
      await useDpiStore.getState().start();
    } catch (e) {
      set({ error: String(e) });
    }

    // 3. Прокси: применяем параметры и запускаем, если бинарник доступен.
    useProxyStore.setState({
      port: profile.proxy_port,
      fakeTlsDomain: profile.fake_tls_domain,
    });
    if (useProxyStore.getState().available) {
      try {
        await useProxyStore.getState().start();
      } catch (e) {
        set({ error: String(e) });
      }
    }

    set({ applyingId: "" });
  },

  clearError: () => set({ error: "" }),
}));
