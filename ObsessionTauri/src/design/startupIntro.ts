import { create } from "zustand";

export const STARTUP_INTRO_KEY = "obsession.intro.seen";
export const STARTUP_INTRO_DURATION = 3420;
export const STARTUP_QUICK_DURATION = 380;
export type StartupIntroMode = "full" | "quick";
export type StartupIntroRequest = { id: number; mode: StartupIntroMode; replay: boolean };

function hasSeenIntro() {
  try { return localStorage.getItem(STARTUP_INTRO_KEY) === "1"; }
  catch { return false; }
}

interface StartupIntroState {
  request: StartupIntroRequest | null;
  arriving: boolean;
  replay(): void;
  arrive(id: number): void;
  finish(id: number, remember?: boolean): void;
}

// Playback belongs to this WebView session. React remounts and theme changes
// cannot accidentally start another launch sequence.
export const useStartupIntro = create<StartupIntroState>((set, get) => ({
  request: { id: 1, mode: hasSeenIntro() ? "quick" : "full", replay: false },
  arriving: false,
  replay: () => set((state) => ({
    request: { id: (state.request?.id ?? nextId) + 1, mode: "full", replay: true },
    arriving: false,
  })),
  arrive: (id) => {
    if (get().request?.id === id) set({ arriving: true });
  },
  finish: (id, remember = true) => {
    if (get().request?.id !== id) return;
    nextId = Math.max(nextId, id);
    if (remember) {
      try { localStorage.setItem(STARTUP_INTRO_KEY, "1"); } catch { /* Optional storage. */ }
    }
    set({ request: null, arriving: false });
  },
}));
let nextId = 1;
