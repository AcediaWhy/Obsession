// Aurora Glass — дизайн-токены. Единый источник цветов/движений для JS-логики
// (Tailwind-конфиг зеркалит эти же значения для классов).

export const colors = {
  base: "#0A0B10",
  accent: "#6366F1",
  accentCyan: "#22D3EE",
  accentViolet: "#8B5CF6",
  ok: "#34D399",
  warn: "#FBBF24",
  danger: "#F87171",
  ink: "#F4F5FB",
  inkSoft: "#B7BCD0",
  inkMuted: "#6B7189",
} as const;

// Общие длительности и пружины интерактивных элементов задаются здесь.
// Фоновые движения тем настраиваются отдельно для каждой сцены.
// CSS-переходы для смены темы и кнопок заданы в globals.css.

// Длительности, сек. Выходы короче входов: уходящее не должно тянуть внимание
// (fast — это ещё и пауза mode="wait" между экранами: держать < ~0.16).
export const dur = {
  fast: 0.14, // выходы экранов/панелей/элементов
  base: 0.3, // входы экранов, оверлеев, шагов визарда
  slow: 0.6, // кроссфейды смены темы (фон и ядро заканчивают ВМЕСТЕ)
} as const;

export const ease = {
  enter: "easeOut", // вход: быстрый старт, мягкое прибытие
  exit: "easeIn", // выход: ускоряется и исчезает
  xfade: "easeInOut", // симметричные кроссфейды сцен
} as const;

// Пружины по РОЛЯМ (а не «скоростям»): выбирая, называй роль элемента.
export const spring = {
  /** Появление/settle крупных поверхностей: панели, модальные иконки. */
  soft: { type: "spring", stiffness: 200, damping: 25 },
  /** Каскадный вход элементов списка/сетки (y + opacity). */
  rise: { type: "spring", stiffness: 240, damping: 26 },
  /** Раскрытие height/layout-перестройки: аккордеоны, въезд карточек. Демпф
   *  выше, чем у rise: overshoot на height читался бы как «пружинит макет». */
  expand: { type: "spring", stiffness: 260, damping: 30 },
  /** Быстрый отклик: тосты, перелёт пилюли навигации. */
  snappy: { type: "spring", stiffness: 340, damping: 30 },
  /** Микроконтролы (ползунок тумблера, hover/tap кнопок): чётко, без желе. */
  flick: { type: "spring", stiffness: 440, damping: 34 },
} as const;

// Каскад детей (Stagger, сетка фич онбординга): шаг между соседями + задержка
// старта. Шаг 0.08 — каскад читается как волна, а не «всё разом».
export const cascade = { step: 0.08, delay: 0.06 } as const;

export type LogLevel = "info" | "success" | "warn" | "error";

export const levelColor: Record<LogLevel, string> = {
  info: colors.inkSoft,
  success: colors.ok,
  warn: colors.warn,
  error: colors.danger,
};
