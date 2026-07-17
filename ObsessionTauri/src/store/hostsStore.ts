import { create } from "zustand";
import {
  api,
  type HostsStatus,
  type VersionedSection,
} from "../lib/tauri";
import { toast } from "./toastStore";
import { useSettingsStore } from "./settingsStore";

type Provider = "malw" | "geohide";

interface HostsState {
  revision: number;
  provider: Provider;
  status: HostsStatus["status"];
  localVersion: string;
  remoteVersion: string;
  busy: boolean;
  error: string;
  rollbackAvailable: boolean;

  applyVersionedStatus: (section: VersionedSection<HostsStatus>) => boolean;
  setProvider: (p: Provider) => Promise<void>;
  refresh: () => Promise<void>;
  install: () => Promise<void>;
  uninstall: () => Promise<void>;
  restore: () => Promise<void>;
  clearError: () => void;
}

export const useHostsStore = create<HostsState>((set, get) => ({
  revision: -1,
  provider: "malw",
  status: "not_installed",
  localVersion: "",
  remoteVersion: "",
  busy: false,
  error: "",
  rollbackAvailable: false,

  applyVersionedStatus: (section) => {
    if (section.revision <= get().revision) return false;
    const status = section.value;
    set({
      revision: section.revision,
      provider: status.provider as Provider,
      status: status.status,
      localVersion: status.local_version,
      remoteVersion: status.remote_version,
      rollbackAvailable: status.rollback_available,
      error: "",
    });
    return true;
  },

  // Меняет провайдера через settingsStore (единый писатель). Оптимистично гасим
  // статус, затем refresh. Внешние писатели (экран Настроек, применение профиля)
  // идут мимо setProvider — их подхватывает общая settings-подписка coordinator.
  setProvider: async (p) => {
    if (p === get().provider) return;
    set({ provider: p, status: "not_installed" });
    await useSettingsStore.getState().patch({ ai_provider: p });
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
        rollbackAvailable: s.rollback_available,
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

  restore: async () => {
    set({ busy: true, error: "" });
    try {
      await api.hostsRestore(get().provider);
      toast.success("Восстановлена последняя рабочая версия hosts");
      await get().refresh();
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  clearError: () => set({ error: "" }),
}));

export function subscribeHostsToSettings(): () => void {
  return useSettingsStore.subscribe((state, previous) => {
    const provider =
      (state.settings?.ai_provider as Provider | undefined) ?? "malw";
    const hosts = useHostsStore.getState();
    if (provider === hosts.provider) return;

    useHostsStore.setState({
      provider,
      status: "not_installed",
      localVersion: "",
      remoteVersion: "",
      rollbackAvailable: false,
    });

    // Initial unified hydration must stay local and fast. Later user/profile
    // provider changes perform the explicit remote refresh.
    if (previous.loaded) void useHostsStore.getState().refresh();
  });
}
