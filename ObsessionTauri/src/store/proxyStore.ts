import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, clipboard, on, type ProxyStatus } from "../lib/tauri";

interface ProxyState {
  available: boolean;
  running: boolean;
  transitioning: boolean;
  link: string;
  lanLink: string | null;
  lanPublished: boolean;
  lanExpiryUnix: number | null;
  port: number;
  fakeTlsDomain: string;
  error: string;
  copied: boolean;

  bootstrap: () => Promise<UnlistenFn>;
  applyStatus: (s: ProxyStatus) => void;
  setPort: (p: number) => void;
  setFakeTlsDomain: (d: string) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  closeLan: () => Promise<void>;
  copy: () => Promise<void>;
  open: () => Promise<void>;
  clearError: () => void;
}

export const useProxyStore = create<ProxyState>((set, get) => ({
  available: false,
  running: false,
  transitioning: false,
  link: "",
  lanLink: null,
  lanPublished: false,
  lanExpiryUnix: null,
  port: 1443,
  fakeTlsDomain: "",
  error: "",
  copied: false,

  bootstrap: async () => {
    const settings = await api.getSettings();
    const available = await api.proxyAvailable();
    set({
      available,
      port: settings.proxy_port,
      fakeTlsDomain: settings.fake_tls_domain,
    });
    // Возвращаем UnlistenFn наверх (App) для отписки — иначе слушатель
    // proxy-status жил бы вечно и дублировался при повторном bootstrap.
    const unlisten = await on.proxyStatus((s) => get().applyStatus(s));
    return unlisten;
  },

  // Применяет статус прокси (из подписки proxy-status ИЛИ из runtime-снапшота при
  // возврате из трея).
  applyStatus: (s) =>
    set({
      running: s.running,
      link: s.link,
      lanLink: s.lan_link,
      lanPublished: s.lan_published,
      lanExpiryUnix: s.lan_expiry_unix,
      transitioning: false,
    }),

  setPort: (p) => set({ port: p }),
  setFakeTlsDomain: (d) => set({ fakeTlsDomain: d }),

  start: async () => {
    if (get().transitioning) return;
    set({ transitioning: true, error: "", link: "", lanLink: null });
    try {
      const link = await api.proxyStart(get().port, get().fakeTlsDomain);
      // lanLink придёт через подписку proxy-status.
      set({ running: true, link, transitioning: false });
      await savePrefs();
    } catch (e) {
      set({ transitioning: false, running: false, error: String(e) });
    }
  },

  stop: async () => {
    if (get().transitioning) return;
    set({ transitioning: true });
    try {
      await api.proxyStop();
    } finally {
      set({ transitioning: false, link: "", lanLink: null, lanPublished: false, lanExpiryUnix: null });
    }
  },

  closeLan: async () => {
    try {
      await api.proxyCloseLan();
      // Финальный статус придёт через подписку proxy-status.
      set({ lanPublished: false, lanExpiryUnix: null, lanLink: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  copy: async () => {
    const { link } = get();
    if (!link) return;
    await clipboard.write(link);
    set({ copied: true });
    setTimeout(() => set({ copied: false }), 1600);
  },

  open: async () => {
    const { link } = get();
    if (!link) return;
    try {
      await api.openExternalUrl(link);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  clearError: () => set({ error: "" }),
}));

async function savePrefs() {
  const { port, fakeTlsDomain } = useProxyStore.getState();
  try {
    await api.updateSettings({
      proxy_port: port,
      fake_tls_domain: fakeTlsDomain,
    });
  } catch {
    /* best-effort */
  }
}
