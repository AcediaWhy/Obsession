import { create } from "zustand";
import { api, type ListInfo } from "../lib/tauri";

interface ListsState {
  lists: ListInfo[];
  loaded: boolean;
  selected: string; // имя выбранного списка ("" — ничего)
  /** Сохранённое на диске содержимое выбранного списка. */
  original: string;
  /** Текущее содержимое в редакторе (может отличаться от original). */
  draft: string;
  loading: boolean; // грузим содержимое выбранного
  saving: boolean;
  error: string;

  bootstrap: () => Promise<void>;
  refresh: () => Promise<void>;
  select: (name: string) => Promise<void>;
  setDraft: (v: string) => void;
  save: () => Promise<void>;
  revert: () => void;
  create: (name: string) => Promise<boolean>;
  remove: (name: string) => Promise<void>;
  clearError: () => void;
}

/** Есть ли несохранённые правки в текущем черновике. */
export function isDirty(s: ListsState): boolean {
  return s.selected !== "" && s.draft !== s.original;
}

export const useListsStore = create<ListsState>((set, get) => ({
  lists: [],
  loaded: false,
  selected: "",
  original: "",
  draft: "",
  loading: false,
  saving: false,
  error: "",

  bootstrap: async () => {
    await get().refresh();
    set({ loaded: true });
    // Автовыбор первого домен-списка, чтобы редактор не был пустым.
    const first = get().lists.find((l) => l.kind === "domains") ?? get().lists[0];
    if (first && !get().selected) await get().select(first.name);
  },

  refresh: async () => {
    try {
      const lists = await api.listsAll();
      set({ lists, error: "" });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  select: async (name) => {
    if (isDirty(get())) {
      // Не теряем правки молча — требуем сохранить/откатить.
      set({ error: "Сначала сохраните или откатите изменения текущего списка." });
      return;
    }
    set({ selected: name, loading: true, error: "" });
    try {
      const content = await api.readList(name);
      // Пока запрос летел, пользователь мог выбрать другой список — не
      // записываем чужое содержимое в редактор текущего.
      if (get().selected !== name) return;
      set({ original: content, draft: content, loading: false });
    } catch (e) {
      if (get().selected !== name) return;
      set({ loading: false, error: String(e) });
    }
  },

  setDraft: (v) => set({ draft: v }),

  save: async () => {
    const { selected, draft } = get();
    if (!selected) return;
    set({ saving: true, error: "" });
    try {
      await api.saveList(selected, draft);
      set({ original: draft, saving: false });
      await get().refresh();
    } catch (e) {
      set({ saving: false, error: String(e) });
    }
  },

  revert: () => set({ draft: get().original, error: "" }),

  create: async (name) => {
    set({ error: "" });
    try {
      const lists = await api.createList(name);
      set({ lists });
      // Переключаемся на новый список (правок нет — select пройдёт).
      set({ selected: "", original: "", draft: "" });
      await get().select(name);
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },

  remove: async (name) => {
    set({ error: "" });
    try {
      const lists = await api.deleteList(name);
      const next: Partial<ListsState> = { lists };
      if (get().selected === name) {
        next.selected = "";
        next.original = "";
        next.draft = "";
      }
      set(next);
      if (!get().selected) {
        const first = get().lists.find((l) => l.kind === "domains") ?? get().lists[0];
        if (first) await get().select(first.name);
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  clearError: () => set({ error: "" }),
}));
