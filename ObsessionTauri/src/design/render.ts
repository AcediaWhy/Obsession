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

import { useSettingsStore } from "../store/settingsStore";

type Listener = (active: boolean) => void;

let active = true;
// Два независимых сигнала: DOM Visibility API (свёрнуто/другой рабочий стол) и
// явный сигнал из Rust (скрытие в трей). Окно «видимо» только когда оба за.
let domVisible = true;
let windowShown = true;
const listeners = new Set<Listener>();

const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
const settingsMotion = () => !useSettingsStore.getState().settings?.reduce_motion;

function compute(): boolean {
  // Пауза, только когда окно реально скрыто (свёрнуто / трей / другой рабочий
  // стол). Простую потерю фокуса скрытием не считаем — иначе анимация замирала
  // бы при каждом клике мимо окна, что выглядит как баг.
  // Также уважаем системную настройку и флаг reduce_motion из настроек.
  return domVisible && windowShown && settingsMotion() && !mediaQuery.matches;
}

function set(next: boolean) {
  if (next === active) return;
  active = next;
  listeners.forEach((cb) => cb(next));
}

if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    domVisible = document.visibilityState !== "hidden";
    set(compute());
  });
  domVisible = document.visibilityState !== "hidden";
  active = compute();

  // Уважаем системную настройку reduce-motion и флаг из настроек приложения.
  mediaQuery.addEventListener("change", () => set(compute()));
  useSettingsStore.subscribe((state, prevState) => {
    if (state.settings?.reduce_motion !== prevState.settings?.reduce_motion) {
      set(compute());
    }
  });
}

/**
 * Сигнал из Rust о скрытии/показе окна в трей. WebView2 не всегда шлёт
 * visibilitychange на `window.hide()`, поэтому дополняем гейт этим сигналом —
 * иначе анимации жгут CPU, пока приложение живёт в трее.
 */
export function setWindowShown(shown: boolean) {
  windowShown = shown;
  set(compute());
}

/** true, если сейчас имеет смысл рисовать кадр. */
export function renderActive(): boolean {
  return active;
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
