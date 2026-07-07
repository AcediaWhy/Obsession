import { create } from "zustand";
import { api, clipboard, on, opener } from "../lib/tauri";

interface ProxyState {
  available: boolean;
  running: boolean;
  transitioning: boolean;
  link: string;
  port: number;
  fakeTlsDomain: string;
  error: string;
  copied: boolean;

  bootstrap: () => Promise<void>;
  setPort: (p: number) => void;
  setFakeTlsDomain: (d: string) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  copy: () => Promise<void>;
  open: () => Promise<void>;
  clearError: () => void;
}

export const useProxyStore = create<ProxyState>((set, get) => ({
  available: false,
  running: false,
  transitioning: false,
  link: "",
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
    on.proxyStatus((s) =>
      set({ running: s.running, link: s.link, transitioning: false }),
    );
  },

  setPort: (p) => set({ port: p }),
  setFakeTlsDomain: (d) => set({ fakeTlsDomain: d }),

  start: async () => {
    if (get().transitioning) return;
    set({ transitioning: true, error: "", link: "" });
    try {
      const link = await api.proxyStart(get().port, get().fakeTlsDomain);
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
      set({ transitioning: false, link: "" });
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
      await opener.open(link);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  clearError: () => set({ error: "" }),
}));

async function savePrefs() {
  const { port, fakeTlsDomain } = useProxyStore.getState();
  try {
    const settings = await api.getSettings();
    await api.saveSettings({
      ...settings,
      proxy_port: port,
      fake_tls_domain: fakeTlsDomain,
    });
  } catch {
    /* best-effort */
  }
}
