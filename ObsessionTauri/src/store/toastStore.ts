import { create } from "zustand";

// Единый канал коротких сообщений (тостов). Заменяет разрозненную инлайн-обратную
// связь: сохранено / ошибка / переподключено и т.п. Автоскрытие по таймеру.

export type ToastKind = "success" | "error" | "info" | "warn";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

interface ToastState {
  toasts: Toast[];
  push: (kind: ToastKind, message: string, ttl?: number) => number;
  dismiss: (id: number) => void;
}

let seq = 0;

export const useToastStore = create<ToastState>((set, get) => ({
  toasts: [],
  push: (kind, message, ttl = 4000) => {
    const id = ++seq;
    // Схлопываем дубли подряд (одинаковый текст) — не спамим стек.
    set((s) => {
      const last = s.toasts[s.toasts.length - 1];
      if (last && last.kind === kind && last.message === message) return s;
      return { toasts: [...s.toasts, { id, kind, message }] };
    });
    if (ttl > 0) setTimeout(() => get().dismiss(id), ttl);
    return id;
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

// Императивный хелпер для вызова вне React-компонентов (сторы, подписки).
export const toast = {
  success: (m: string, ttl?: number) => useToastStore.getState().push("success", m, ttl),
  error: (m: string, ttl?: number) => useToastStore.getState().push("error", m, ttl),
  info: (m: string, ttl?: number) => useToastStore.getState().push("info", m, ttl),
  warn: (m: string, ttl?: number) => useToastStore.getState().push("warn", m, ttl),
};
