import { create } from "zustand";

// Система пасхалок: ввод кодовых слов открывает мелкие бонусы.
//  • theme   — разблокирует скрытую тему (появляется в выборе оформления).
//  • overlay — включает/выключает визуальный оверлей поверх любой темы
//              (снег, сакура). Повторный ввод кода — переключает.
// Разблокировки и состояние оверлеев хранятся в localStorage. Расширяется
// добавлением записи в REGISTRY.

type RewardKind = "theme" | "overlay";
type Reward = {
  id: string;
  kind: RewardKind;
  title: string;
  keys: string[]; // нормализованные варианты кода (латиница/кириллица/цифры)
};

// Нормализация: нижний регистр, убираем всё кроме букв (любой алфавит) и цифр.
// "Fallen Down"→"fallendown", "Россия"→"россия".
function norm(s: string): string {
  return s.toLowerCase().normalize("NFC").replace(/[^\p{L}\p{N}]/gu, "");
}

const REGISTRY: Reward[] = [
  { id: "fallendown", kind: "theme", title: "Fallen Down", keys: ["fallendown"] },
  { id: "russia", kind: "theme", title: "Russia", keys: ["russia", "россия"] },
  { id: "snow", kind: "overlay", title: "Снег", keys: ["snow", "снег"] },
  { id: "sakura", kind: "overlay", title: "Сакура", keys: ["sakura", "сакура"] },
];

export type OverlayId = "snow" | "sakura";

export type RedeemResult =
  | { status: "unlocked"; kind: RewardKind; id: string; title: string }
  | { status: "already"; kind: RewardKind; id: string; title: string }
  | { status: "on"; kind: "overlay"; id: string; title: string }
  | { status: "off"; kind: "overlay"; id: string; title: string }
  | { status: "unknown" };

const KEY_UNLOCKED = "obsession.secrets";
const KEY_OVERLAYS = "obsession.overlays";

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
  overlays: Record<OverlayId, boolean>;
  isUnlocked: (id: string) => boolean;
  redeem: (input: string) => RedeemResult;
  setOverlay: (id: OverlayId, on: boolean) => void;
}

function initOverlays(): Record<OverlayId, boolean> {
  const active = loadArr(KEY_OVERLAYS);
  return { snow: active.includes("snow"), sakura: active.includes("sakura") };
}

export const useSecretStore = create<SecretState>((set, get) => ({
  unlocked: loadArr(KEY_UNLOCKED),
  overlays: initOverlays(),

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

    if (reward.kind === "overlay") {
      const oid = reward.id as OverlayId;
      const next = !get().overlays[oid];
      get().setOverlay(oid, next);
      return { status: next ? "on" : "off", kind: "overlay", id: reward.id, title: reward.title };
    }

    // theme: одноразовая разблокировка.
    return {
      status: wasKnown ? "already" : "unlocked",
      kind: reward.kind,
      id: reward.id,
      title: reward.title,
    };
  },

  setOverlay: (id, on) => {
    const overlays = { ...get().overlays, [id]: on };
    set({ overlays });
    const active = (Object.keys(overlays) as OverlayId[]).filter((k) => overlays[k]);
    persist(KEY_OVERLAYS, active);
  },
}));
