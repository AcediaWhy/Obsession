import { isTauri } from "@tauri-apps/api/core";
import { create } from "zustand";

import { api, on, type UpdateCheck } from "../lib/tauri";
import { toast } from "./toastStore";

type Phase = "idle" | "checking" | "downloading" | "launching" | "error";

interface UpdateState {
  phase: Phase;
  result: UpdateCheck | null;
  error: string | null;
  downloaded: number;
  total: number;
  check: (automatic?: boolean) => Promise<void>;
  install: () => Promise<void>;
}

const LAST_CHECK_KEY = `obsession.update.last-check.${__APP_VERSION__}`;
const LAST_ATTEMPT_KEY = `obsession.update.last-attempt.${__APP_VERSION__}`;
const RESULT_KEY = "obsession.update.last-result";
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

function cachedResult(): UpdateCheck | null {
  try {
    const raw = localStorage.getItem(RESULT_KEY);
    if (!raw) return null;
    const value = JSON.parse(raw) as UpdateCheck;
    return value.current_version === __APP_VERSION__
      && typeof value.latest_version === "string"
      && typeof value.available === "boolean"
      && typeof value.notes === "string"
      && typeof value.release_url === "string"
      ? value : null;
  } catch {
    return null;
  }
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  phase: "idle",
  result: cachedResult(),
  error: null,
  downloaded: 0,
  total: 0,
  check: async (automatic = false) => {
    if (get().phase === "checking" || get().phase === "downloading" || get().phase === "launching") return;
    if (automatic) {
      if (!isTauri()) return;
      try {
        const last = Number(localStorage.getItem(LAST_CHECK_KEY) || 0);
        const attempt = Number(localStorage.getItem(LAST_ATTEMPT_KEY) || 0);
        if (Date.now() - last < CHECK_INTERVAL_MS || Date.now() - attempt < 15 * 60 * 1000) return;
        localStorage.setItem(LAST_ATTEMPT_KEY, String(Date.now()));
      } catch {
        // Storage may be disabled; a failed cache write must not block checking.
      }
    }
    set({ phase: "checking", error: null });
    try {
      const result = await api.updateCheck();
      const previous = get().result;
      set({ phase: "idle", result, error: null });
      try {
        localStorage.setItem(RESULT_KEY, JSON.stringify(result));
        localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
      } catch { /* optional cache */ }
      if (automatic && result.available && previous?.latest_version !== result.latest_version) {
        toast.info(`Доступна Obsession ${result.latest_version}. Обновить можно в настройках.`, 9000);
      }
    } catch (error) {
      const message = String(error);
      set({ phase: automatic ? "idle" : "error", error: automatic ? null : message });
    }
  },
  install: async () => {
    if (!get().result?.available || (get().phase !== "idle" && get().phase !== "error")) return;
    set({ phase: "downloading", downloaded: 0, total: 0, error: null });
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await on.updateProgress(({ downloaded, total }) => {
        set({ downloaded, total });
      });
      await api.updateInstall();
      set({ phase: "launching" });
    } catch (error) {
      set({ phase: "error", error: String(error) });
    } finally {
      unlisten?.();
    }
  },
}));

export function startUpdateChecks(): () => void {
  const initial = window.setTimeout(() => void useUpdateStore.getState().check(true), 8000);
  const interval = window.setInterval(() => void useUpdateStore.getState().check(true), 60 * 60 * 1000);
  return () => {
    window.clearTimeout(initial);
    window.clearInterval(interval);
  };
}
