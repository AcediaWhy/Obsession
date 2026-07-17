// Общий флаг «рендер активен»: когда окно свёрнуто/скрыто, canvas-циклы
// приостанавливают отрисовку. Это заметно экономит CPU/GPU (главный источник
// нагрузки — анимированные canvas под backdrop-filter в WebView2).
//
// Использование в rAF-цикле (2D-темы):
//   if (!renderActive()) { last = 0; return; }   // last=0 → без скачка dt
// Сбрасывать last при паузе обязательно: иначе после долгой паузы первый dt
// окажется огромным и анимация «прыгнет».
//
// Для R3F-сцен (WebGL) синхронный геттер не годится — React должен реагировать
// на изменение, чтобы переключить <Canvas frameloop>. Для этого есть подписка
// onRenderActiveChange, единый источник правды со 2D-темами.

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
// Два независимых сигнала: DOM Visibility API (свёрнуто/другой рабочий стол) и
// явный сигнал из Rust (скрытие в трей). Окно «видимо» только когда оба за.
let domVisible = true;
let windowShown = true;
// Пауза раскладывается на ДВЕ независимые причины (важно для стоп-кадра):
//  • hidden — окно реально скрыто (трей/свёрнуто/другой стол): не рисуем вовсе.
//    Простую потерю фокуса скрытием не считаем — иначе анимация замирала бы
//    при каждом клике мимо окна, что выглядит как баг.
//  • still  — reduce-motion (флаг настроек ИЛИ системный prefers-reduced-motion):
//    цикл рисует РОВНО ОДИН кадр и замирает — canvas-темы выглядят статичными
//    постерами (глаз/сердце/солнце на месте), а не пустотой.
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
  // Уведомляем на любую смену пары (hidden, still), а не только active: циклам
  // нужно отличать «скрылись» от «замерли со стоп-кадром» (см. createRenderLoop).
  // Повторный вызов с тем же active для остальных подписчиков безвреден.
  listeners.forEach((cb) => cb(active));
}

if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    domVisible = document.visibilityState !== "hidden";
    recompute();
  });
  domVisible = document.visibilityState !== "hidden";
  recompute();

  // Уважаем системную настройку reduce-motion и флаг из настроек приложения.
  mediaQuery.addEventListener("change", recompute);
  useSettingsStore.subscribe((state, prevState) => {
    if (state.settings?.reduce_motion !== prevState.settings?.reduce_motion) {
      recompute();
    }
  });
}

/**
 * Сигнал из Rust о скрытии/показе окна в трей. WebView2 не всегда шлёт
 * visibilitychange на `window.hide()`, поэтому дополняем гейт этим сигналом —
 * иначе анимации жгут CPU, пока приложение живёт в трее.
 *
 * Дополнительно подменяем `document.hidden`/`visibilityState` и диспатчим
 * `visibilitychange`: так framer-motion (свой rAF-движок) и CSS-анимации
 * узнают о скрытии и останавливаются. Иначе они крутятся в трее на полной
 * скорости, т.к. слушают Visibility API, а не наш сигнал.
 */
export function setWindowShown(shown: boolean) {
  windowShown = shown;
  const doc = document as unknown as Record<string, unknown>;
  if (shown) {
    // Восстанавливаем прототипные геттеры (удаляем наши override'ы).
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
// FrameScheduler выбирает cadence по роли и измеренной герцовке. Rain пока
// сохраняет отдельный совместимый cap до объединения pipeline в Task 3.3.
export const CORE_HERO_MIN_SIZE = 160;
export const FPS_RAIN = 60;

export type RenderLoop = SchedulerFrameLoop;

/**
 * Compatibility adapter к общему FrameScheduler: все canvas-темы делят один
 * rAF, visibility/reduced-motion gate, cadence и dt policy. Публичный контракт
 * старых циклов сохраняется до миграции ролей/quality subscriptions.
 *
 * Троттлинг — по сетке периодов: `nextDue` шагает кратно 1000/fps, а не от
 * времени последнего кадра (`last = now` занижает fps и даёт неровную каденцию
 * на герцовках, не кратных капу). Допуск `eps` спасает от «ножевого края»,
 * когда порог совпадает с интервалом кадра (60 fps на 60/120 Гц): rAF-таймстамп
 * приходит на доли мс раньше дедлайна, и без допуска кадр дропается — fps
 * проваливается вдвое.
 *
 * dt — из реального времени с клампом maxDt: скорость анимаций не зависит от
 * fps. Кламп 0.25 — только страховка от «телепорта» после настоящего фриза
 * (окно секунду стояло): при обычных просадках dt проходит как есть, анимация
 * держит реальную скорость (кадры пропускаются, но время не замедляется).
 * Низкий кламп (≤0.1) превращал каждую просадку в слоу-мо.
 */
export function createRenderLoop(
  draw: (dt: number, now: number) => void,
  opts: SchedulerFrameLoopOptions = {},
): RenderLoop {
  return frameScheduler.createLoop(draw, opts);
}
