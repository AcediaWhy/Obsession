import { create } from "zustand";
import {
  api,
  clipboard,
  type ProxyStatus,
  type Settings,
  type VersionedSection,
} from "../lib/tauri";

interface ProxyState {
  revision: number;
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

  initialize: (settings: Settings, available: boolean) => void;
  applyVersionedStatus: (section: VersionedSection<ProxyStatus>) => boolean;
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

function statusPatch(status: ProxyStatus) {
  return {
    running: status.running,
    link: status.link,
    lanLink: status.lan_link,
    lanPublished: status.lan_published,
    lanExpiryUnix: status.lan_expiry_unix,
  };
}

export const useProxyStore = create<ProxyState>((set, get) => ({
  revision: -1,
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

  initialize: (settings, available) => {
    set({
      available,
      port: settings.proxy_port,
      fakeTlsDomain: settings.fake_tls_domain,
      error: "",
    });
  },

  // Применяет статус прокси (из подписки proxy-status ИЛИ из runtime-снапшота при
  // возврате из трея). НЕ трогает transitioning: им владеют start()/stop() (как в
  // dpiStore). Иначе ранний proxy-status во время штатного start/stop снимал бы
  // блокировку кнопки до resolve invoke → повторный клик ловил гонку и «running:
  // false + error» при живом прокси.
  applyVersionedStatus: (section) => {
    if (section.revision <= get().revision) return false;
    set({ ...statusPatch(section.value), revision: section.revision });
    return true;
  },

  applyStatus: (status) => set(statusPatch(status)),

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
