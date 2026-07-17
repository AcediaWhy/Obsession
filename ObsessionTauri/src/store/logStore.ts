import { create } from "zustand";

import { on, type LogEvent } from "../lib/tauri";
import { frameScheduler } from "../design/frameScheduler";

const MAX = 200;

export interface LogLine extends LogEvent {
  id: number;
}

interface LogState {
  lines: LogLine[];
  push: (event: LogEvent) => void;
  clear: () => void;
}

let nextLogId = 1;
let flushStarted = false;
const pending: LogLine[] = [];

function flushPendingLogs(): void {
  if (pending.length === 0) return;
  const batch = pending.splice(0, pending.length);
  useLogStore.setState((state) => ({
    lines: [...state.lines, ...batch].slice(-MAX),
  }));
}

const flushLoop = frameScheduler.createLoop(flushPendingLogs, {
  paused: true,
  role: "secondary",
});

function scheduleFlush(): void {
  if (!flushStarted) {
    flushStarted = true;
    flushLoop.start();
  } else {
    flushLoop.invalidate();
  }
}

export const useLogStore = create<LogState>((set) => ({
  lines: [],
  push: (event) => {
    pending.push({ ...event, id: nextLogId++ });
    scheduleFlush();
  },
  clear: () => {
    pending.length = 0;
    set({ lines: [] });
  },
}));

// Подписка на лог-события бэкенда (вызывается один раз при старте).
export function initLogStream() {
  return on.log((event) => useLogStore.getState().push(event));
}
