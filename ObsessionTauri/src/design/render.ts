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

type Listener = (active: boolean) => void;

let active = true;
const listeners = new Set<Listener>();

function compute(): boolean {
  // Пауза, только когда окно реально скрыто (свёрнуто / другой рабочий стол).
  // Простую потерю фокуса скрытием не считаем — иначе анимация замирала бы при
  // каждом клике мимо окна, что выглядит как баг.
  return document.visibilityState !== "hidden";
}

function set(next: boolean) {
  if (next === active) return;
  active = next;
  listeners.forEach((cb) => cb(next));
}

if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => set(compute()));
  active = compute();
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
