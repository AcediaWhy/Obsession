import type { Variants } from "framer-motion";

import { dur, ease } from "./tokens";

// Направленный слайд экранов: контент движется в сторону перехода по меню
// (вниз по списку — уходит вверх, вверх — вниз). Направление приходит через
// custom: у уходящего экрана пропсы заморожены AnimatePresence, и только
// custom на самом AnimatePresence обновляется для его exit-варианта.
// Обёртка transform-only (без opacity) — см. комментарий у <motion.div key={tab}>.
//
// Вынесено из App отдельным модулем, чтобы pointerEvents-контракт (ниже)
// покрывался юнит-тестом: баг тихий, глазами в ревью не ловится.
export const screenVariants: Variants = {
  // 8px — дистанция page-side-by-side из transitions.dev: направление читается,
  // но экран не складывает большой пробег с локальными layout-анимациями.
  enter: (dir: number) => ({ y: 8 * dir, pointerEvents: "auto" }),
  // pointerEvents задаём явно и здесь: прерванный выход (быстрый возврат на тот
  // же раздел) оживляет ТОТ ЖЕ элемент, и без сброса на нём навсегда осталось бы
  // "none" из exit-варианта — экран стал бы некликабельным.
  center: { y: 0, pointerEvents: "auto" },
  exit: (dir: number) => ({
    y: -6 * dir,
    // Уходящий экран — absolute inset-0 поверх всей контентной области, и
    // AnimatePresence держит его смонтированным, пока не завершатся exit-анимации
    // ВСЕХ потомков. Панели гаснут через opacity (GlassPanel), а нулевая
    // прозрачность НЕ отключает хит-тестинг — невидимый экран продолжал ловить
    // клики по новому (симптом: после «Настройки → другой раздел → Настройки»
    // не переключалась тема). Гасим pointerEvents сразу, как у уходящего ядра
    // в HeroCore.
    pointerEvents: "none",
    transition: { duration: dur.fast, ease: ease.exit },
  }),
};
