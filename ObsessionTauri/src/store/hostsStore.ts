import { create } from "zustand";
import { api, type HostsStatus } from "../lib/tauri";
import { toast } from "./toastStore";

type Provider = "malw" | "geohide";

interface HostsState {
  provider: Provider;
  status: HostsStatus["status"];
  localVersion: string;
  remoteVersion: string;
  busy: boolean;
  error: string;

  bootstrap: () => Promise<void>;
  setProvider: (p: Provider) => Promise<void>;
  refresh: () => Promise<void>;
  install: () => Promise<void>;
  uninstall: () => Promise<void>;
  clearError: () => void;
}

export const useHostsStore = create<HostsState>((set, get) => ({
  provider: "malw",
  status: "not_installed",
  localVersion: "",
  remoteVersion: "",
  busy: false,
  error: "",

  bootstrap: async () => {
    const settings = await api.getSettings();
    set({ provider: (settings.ai_provider as Provider) ?? "malw" });
    await get().refresh();
    if (get().status === "outdated") {
      toast.warn("Доступно обновление ИИ-хостов", 6000);
    }
  },

  setProvider: async (p) => {
    set({ provider: p, status: "not_installed" });
    const settings = await api.getSettings();
    await api.saveSettings({ ...settings, ai_provider: p });
    await get().refresh();
  },

  refresh: async () => {
    set({ busy: true, error: "" });
    try {
      const s = await api.hostsStatus(get().provider);
      set({
        status: s.status,
        localVersion: s.local_version,
        remoteVersion: s.remote_version,
        busy: false,
      });
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  install: async () => {
    set({ busy: true, error: "" });
    try {
      await api.hostsInstall(get().provider);
      await get().refresh();
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  uninstall: async () => {
    set({ busy: true, error: "" });
    try {
      await api.hostsUninstall();
      await get().refresh();
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  clearError: () => set({ error: "" }),
}));
