// Общий флаг «рендер активен»: когда окно свёрнуто/скрыто, canvas-циклы
// приостанавливают отрисовку. Это заметно экономит CPU/GPU (главный источник
// нагрузки — анимированные canvas под backdrop-filter в WebView2).
//
// Использование в rAF-цикле:
//   if (!renderActive()) { last = 0; return; }   // last=0 → без скачка dt
// Сбрасывать last при паузе обязательно: иначе после долгой паузы первый dt
// окажется огромным и анимация «прыгнет».

let active = true;

function compute(): boolean {
  // Пауза, только когда окно реально скрыто (свёрнуто / другой рабочий стол).
  // Простую потерю фокуса скрытием не считаем — иначе анимация замирала бы при
  // каждом клике мимо окна, что выглядит как баг.
  return document.visibilityState !== "hidden";
}

if (typeof document !== "undefined") {
  const update = () => {
    active = compute();
  };
  document.addEventListener("visibilitychange", update);
  active = compute();
}

/** true, если сейчас имеет смысл рисовать кадр. */
export function renderActive(): boolean {
  return active;
}
