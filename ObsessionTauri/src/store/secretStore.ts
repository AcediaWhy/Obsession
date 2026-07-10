import { create } from "zustand";

// Система пасхалок: ввод кодовых слов открывает скрытые темы (появляются в
// выборе оформления). Разблокировки хранятся в localStorage. Расширяется
// добавлением записи в REGISTRY.

type Reward = {
  id: string;
  title: string;
  keys: string[]; // нормализованные варианты кода (латиница/кириллица/цифры)
};

// Нормализация: нижний регистр, убираем всё кроме букв (любой алфавит) и цифр.
// "Fallen Down"→"fallendown", "Россия"→"россия".
function norm(s: string): string {
  return s.toLowerCase().normalize("NFC").replace(/[^\p{L}\p{N}]/gu, "");
}

const REGISTRY: Reward[] = [
  { id: "fallendown", title: "Fallen Down", keys: ["fallendown"] },
  { id: "russia", title: "Russia", keys: ["russia", "россия"] },
];

export type RedeemResult =
  | { status: "unlocked"; id: string; title: string }
  | { status: "already"; id: string; title: string }
  | { status: "unknown" };

const KEY_UNLOCKED = "obsession.secrets";

function loadArr(key: string): string[] {
  try {
    const raw = localStorage.getItem(key);
    if (raw) {
      const arr = JSON.parse(raw);
      if (Array.isArray(arr)) return arr.filter((x) => typeof x === "string");
    }
  } catch {
    /* повреждённое хранилище — с чистого листа */
  }
  return [];
}

function persist(key: string, val: string[]) {
  try {
    localStorage.setItem(key, JSON.stringify(val));
  } catch {
    /* игнорируем */
  }
}

interface SecretState {
  unlocked: string[]; // открытые id (для видимости тем и «обнаружено»)
  isUnlocked: (id: string) => boolean;
  redeem: (input: string) => RedeemResult;
}

export const useSecretStore = create<SecretState>((set, get) => ({
  unlocked: loadArr(KEY_UNLOCKED),

  isUnlocked: (id) => get().unlocked.includes(id),

  redeem: (input) => {
    const key = norm(input);
    if (!key) return { status: "unknown" };
    const reward = REGISTRY.find((r) => r.keys.includes(key));
    if (!reward) return { status: "unknown" };

    const wasKnown = get().unlocked.includes(reward.id);
    if (!wasKnown) {
      const unlocked = [...get().unlocked, reward.id];
      persist(KEY_UNLOCKED, unlocked);
      set({ unlocked });
    }

    return {
      status: wasKnown ? "already" : "unlocked",
      id: reward.id,
      title: reward.title,
    };
  },
}));
