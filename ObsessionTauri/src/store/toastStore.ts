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
  /** Курсор над тостом: замораживаем автоскрытие, пока читают. */
  pause: (id: number) => void;
  /** Курсор ушёл: дочитали — досчитываем ОСТАТОК ttl, не полный заново. */
  resume: (id: number) => void;
}

let seq = 0;

// Живые таймеры автоскрытия: id тоста → таймер + дедлайн (для остатка при
// паузе). Вне стора: это не состояние UI, подписчикам они не нужны.
const timers = new Map<number, { handle: ReturnType<typeof setTimeout>; deadline: number; remaining?: number }>();

function arm(id: number, ms: number, dismiss: (id: number) => void) {
  timers.set(id, {
    handle: setTimeout(() => {
      timers.delete(id);
      dismiss(id);
    }, ms),
    deadline: Date.now() + ms,
  });
}

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
    if (ttl > 0) arm(id, ttl, get().dismiss);
    return id;
  },
  dismiss: (id) => {
    const t = timers.get(id);
    if (t) {
      clearTimeout(t.handle);
      timers.delete(id);
    }
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
  pause: (id) => {
    const t = timers.get(id);
    if (!t || t.remaining !== undefined) return;
    clearTimeout(t.handle);
    // Минимум 1с на «дочитать» после ухода курсора — резюм с 50мс остатка
    // выглядел бы как «тост убежал из-под мыши».
    t.remaining = Math.max(1000, t.deadline - Date.now());
  },
  resume: (id) => {
    const t = timers.get(id);
    if (!t || t.remaining === undefined) return;
    const ms = t.remaining;
    timers.delete(id);
    arm(id, ms, get().dismiss);
  },
}));

// Императивный хелпер для вызова вне React-компонентов (сторы, подписки).
export const toast = {
  success: (m: string, ttl?: number) => useToastStore.getState().push("success", m, ttl),
  error: (m: string, ttl?: number) => useToastStore.getState().push("error", m, ttl),
  info: (m: string, ttl?: number) => useToastStore.getState().push("info", m, ttl),
  warn: (m: string, ttl?: number) => useToastStore.getState().push("warn", m, ttl),
};
