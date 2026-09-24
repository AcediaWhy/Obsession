import type { Variants } from "framer-motion";

import { dur, ease } from "./tokens";

// Направление перехода передаётся через AnimatePresence.custom: свойства
// уходящего экрана остаются замороженными до завершения анимации.
export const screenVariants: Variants = {
  // Небольшой сдвиг не конфликтует с анимациями внутри страницы.
  enter: (dir: number) => ({ y: 8 * dir, pointerEvents: "auto" }),
  // После прерванного выхода возвращаем странице обработку указателя.
  center: {
    y: 0,
    pointerEvents: "auto",
    // Tween не даёт всей странице выходить за конечную позицию.
    transition: { type: "tween", duration: dur.base, ease: [0.22, 1, 0.36, 1] },
  },
  exit: (dir: number) => ({
    y: -6 * dir,
    // Невидимый уходящий экран остаётся смонтированным и иначе перехватывает клики.
    pointerEvents: "none",
    transition: { duration: dur.fast, ease: ease.exit },
  }),
};
