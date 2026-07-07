import { create } from "zustand";
import { on, type LogEvent } from "../lib/tauri";

const MAX = 200;

interface LogState {
  lines: LogEvent[];
  push: (e: LogEvent) => void;
  clear: () => void;
}

export const useLogStore = create<LogState>((set) => ({
  lines: [],
  push: (e) =>
    set((s) => ({ lines: [...s.lines.slice(-(MAX - 1)), e] })),
  clear: () => set({ lines: [] }),
}));

// Подписка на лог-события бэкенда (вызывается один раз при старте).
export function initLogStream() {
  return on.log((e) => useLogStore.getState().push(e));
}
