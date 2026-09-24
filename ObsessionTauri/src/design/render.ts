// Управляет отрисовкой при скрытии окна и при reduced-motion.
// Скрытое окно не рисует кадры; reduced-motion оставляет один стоп-кадр.
// Подписка сообщает об изменении состояния React-компонентам и canvas-циклам.

import { useSyncExternalStore } from "react";

import { useSettingsStore } from "../store/settingsStore";
import {
  frameScheduler,
  type FrameLoop as SchedulerFrameLoop,
  type FrameLoopOptions as SchedulerFrameLoopOptions,
} from "./frameScheduler";

export { frameQualityScale } from "./frameScheduler";
export type { FrameRole, QualityTier } from "./frameScheduler";

type Listener = (active: boolean) => void;

let active = true;
// Окно видно, только если его показывают и DOM Visibility API, и Tauri.
let domVisible = true;
let windowShown = true;
// hidden приостанавливает отрисовку; still сохраняет один кадр при reduced-motion.
let hidden = false;
let still = false;
const listeners = new Set<Listener>();

const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
const settingsMotion = () => !useSettingsStore.getState().settings?.reduce_motion;

function recompute() {
  const nextHidden = !(domVisible && windowShown);
  const nextStill = !settingsMotion() || mediaQuery.matches;
  if (nextHidden === hidden && nextStill === still) return;
  hidden = nextHidden;
  still = nextStill;
  active = !hidden && !still;
  frameScheduler.setRenderState({ hidden, reducedMotion: still });
  // Оповещаем и при смене причины паузы: циклам нужно различать hidden и still.
  listeners.forEach((cb) => cb(active));
}

if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    domVisible = document.visibilityState !== "hidden";
    recompute();
  });
  domVisible = document.visibilityState !== "hidden";
  recompute();

  // Системная и пользовательская настройки движения имеют одинаковый приоритет.
  mediaQuery.addEventListener("change", recompute);
  useSettingsStore.subscribe((state, prevState) => {
    if (state.settings?.reduce_motion !== prevState.settings?.reduce_motion) {
      recompute();
    }
  });
}

/**
 * Принимает сигнал Tauri о скрытии окна. WebView2 может не отправить
 * `visibilitychange` при `window.hide()`, поэтому обновляем Visibility API
 * и отправляем событие для анимаций, которые на него подписаны.
 */
export function setWindowShown(shown: boolean) {
  windowShown = shown;
  const doc = document as unknown as Record<string, unknown>;
  if (shown) {
    // Удаляем локальные свойства, чтобы снова действовали геттеры прототипа.
    delete doc.hidden;
    delete doc.visibilityState;
    (document.documentElement as HTMLElement).removeAttribute("data-hidden");
  } else {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
    document.documentElement.setAttribute("data-hidden", "true");
  }
  document.dispatchEvent(new Event("visibilitychange"));
  recompute();
}

/** true, если сейчас имеет смысл рисовать кадры непрерывно. */
export function renderActive(): boolean {
  return active;
}

/** true, если окно реально скрыто (трей/свёрнуто) — не рисовать вовсе. */
export function renderHidden(): boolean {
  return hidden;
}

/** true, если включён reduce-motion — рисуется один стоп-кадр. */
export function renderStill(): boolean {
  return still;
}

/**
 * React-хук «анимации отключены» (флаг настроек ИЛИ системный
 * prefers-reduced-motion). Для framer-переходов, которые reducedMotion="always"
 * не глушит (opacity, height): transition={off ? { duration: 0 } : ...}.
 */
export function useMotionOff(): boolean {
  return useSyncExternalStore(onRenderActiveChange, renderStill, renderStill);
}

/**
 * Подписка на изменение флага видимости. Возвращает функцию отписки.
 * Колбэк вызывается только при смене состояния (не на каждый кадр).
 */
export function onRenderActiveChange(cb: Listener): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/**
 * React-хук: `true`, пока окно видимо. Перерисовывает компонент при смене флага.
 * Нужен, чтобы гасить бесконечные framer-motion-анимации (`repeat: Infinity`) в
 * трее: WebView2 не сообщает странице о скрытии окна, поэтому WAAPI-анимации
 * иначе крутятся на полной скорости и жгут CPU в свёрнутом состоянии.
 */
export function useRenderActive(): boolean {
  return useSyncExternalStore(onRenderActiveChange, renderActive, renderActive);
}

/**
 * React-хук: `true`, когда окно РЕАЛЬНО скрыто (трей/свёрнуто/другой стол). В
 * отличие от useRenderActive, НЕ реагирует на reduce-motion — под reduce-motion
 * сцена рисует стоп-кадр (постер), а не размонтируется. По этому сигналу
 * suspend-разгрузка снимает тяжёлую визуалку (сцены тем, ядра, video-декодеры,
 * canvas backing stores), сохраняя контроллер/сторы/подписки/черновики форм.
 */
export function useRenderHidden(): boolean {
  return useSyncExternalStore(onRenderActiveChange, renderHidden, renderHidden);
}

// ─── Политика частоты кадров тем ─────────────────────────────────────────────
// FrameScheduler выбирает cadence по роли и измеренной герцовке.
export const CORE_HERO_MIN_SIZE = 160;

export type RenderLoop = SchedulerFrameLoop;

/**
 * Совместимый интерфейс canvas-циклов. Частоту кадров, состояние видимости
 * и шаг времени определяет общий FrameScheduler.
 */
export function createRenderLoop(
  draw: (dt: number, now: number) => void,
  opts: SchedulerFrameLoopOptions = {},
): RenderLoop {
  return frameScheduler.createLoop(draw, opts);
}
