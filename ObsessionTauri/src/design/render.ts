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

// ─── Политика частоты кадров тем ─────────────────────────────────────────────
// Единственное место, где крутятся fps-капы. 0 = без капа (нативная герцовка).
//
// 2D-темы и ядра идут без капа ради плавности: на 120–180 Гц кап 60 не делит
// герцовку ровно (60∤144) — кадры шагают неровно 14/21 мс и читаются как
// «замедленные». CPU в трее это не трогает: циклы полностью встают по
// renderActive. Дождь — исключение: тяжёлая полноэкранная WebGL-сцена, без
// капа греет 2–4% CPU на ЛЮБОМ экране, пока окно открыто (проверено вживую).
export const FPS_FIELD = 0; // полноэкранные фоны — нативная герцовка
export const FPS_CORE_HERO = 0; // ядро-«глаз» (size >= 160, экраны Dpi/Telegram)
export const FPS_CORE_PREVIEW = 0; // мини-превью в Настройках (живых максимум два)
export const CORE_HERO_MIN_SIZE = 160;
export const coreFps = (size: number) =>
  size >= CORE_HERO_MIN_SIZE ? FPS_CORE_HERO : FPS_CORE_PREVIEW;
// Дождь (WebGL): рендер и симуляция капель на одном капе.
export const FPS_RAIN = 60;

export type RenderLoop = {
  /** Идемпотентен. При paused рисует один кадр и замирает. */
  start(): void;
  /** Глушит rAF; внутреннее состояние не сбрасывает. */
  stop(): void;
  /** true: дорисовать один кадр и замереть; false: продолжить без сброса состояния. */
  setPaused(paused: boolean): void;
  /** В paused-режиме перерисовать один кадр (смена пропсов active/busy). Иначе no-op. */
  invalidate(): void;
  /** Отписка от гейта видимости + cancel. После этого объект мёртв. */
  dispose(): void;
};

/**
 * Единый rAF-цикл для canvas-тем: гейт видимости, кап fps и dt в одном месте
 * (раньше эта обвязка копипастилась в ~12 компонентах — с расползающимися
 * багами троттлинга).
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
  opts: { fps?: number; maxDt?: number; paused?: boolean } = {},
): RenderLoop {
  const frame = opts.fps && opts.fps > 0 ? 1000 / opts.fps : 0; // 0 = без капа
  const eps = frame ? Math.min(2, frame * 0.25) : 0;
  const maxDt = opts.maxDt ?? 0.25;

  let raf = 0;
  let lastDraw = 0; // время предыдущего НАРИСОВАННОГО кадра — для dt
  let nextDue = 0; // дедлайн следующего кадра по fps-сетке — для троттла
  let paused = !!opts.paused;
  let disposed = false;

  const tick = (now: number) => {
    raf = 0;
    if (disposed) return;
    if (renderHidden()) {
      // Окно скрыто: не рисуем; lastDraw=0 → после паузы dt не «прыгнет».
      lastDraw = 0;
      nextDue = 0;
      return;
    }
    if (frame && now < nextDue - eps) {
      raf = requestAnimationFrame(tick); // рано — ждём следующий vsync
      return;
    }
    const dt = lastDraw ? Math.min((now - lastDraw) / 1000, maxDt) : (frame || 16.7) / 1000;
    lastDraw = now;
    // Дедлайн шагает по сетке периодов (среднее = ровно fps на любой герцовке);
    // после лага/паузы — ресинк от now, без burst-догона.
    if (frame) nextDue = nextDue + frame > now ? nextDue + frame : now + frame;
    draw(dt, now);
    // paused/reduce-motion: один кадр — и замерли (raf уже 0). Под reduce-motion
    // сцена остаётся видимым статичным «постером», а не пустым канвасом.
    if (paused || renderStill()) return;
    raf = requestAnimationFrame(tick);
  };

  const start = () => {
    if (!disposed && !raf && !renderHidden()) raf = requestAnimationFrame(tick);
  };
  const stop = () => {
    if (raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
  };
  const unsub = onRenderActiveChange(() => {
    if (renderHidden()) {
      stop();
      return;
    }
    // Видимы: продолжаем (или, под reduce-motion, дорисуем один стоп-кадр —
    // tick сам замрёт). lastDraw=0 → возврат из любой паузы без скачка dt.
    lastDraw = 0;
    start();
  });

  return {
    start,
    stop,
    setPaused(p: boolean) {
      if (p === paused) return;
      paused = p;
      if (!p) lastDraw = 0; // продолжение после долгой паузы — без скачка dt
      start(); // paused=true: дорисовать «застывший» кадр; false: продолжить
    },
    invalidate() {
      if (paused || renderStill()) start();
    },
    dispose() {
      disposed = true;
      unsub();
      stop();
    },
  };
}
