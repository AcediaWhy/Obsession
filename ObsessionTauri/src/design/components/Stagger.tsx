import { motion, type Variants } from "framer-motion";
import type { ReactNode } from "react";

import { cascade, dur, ease, spring } from "../tokens";

// Каскадное появление: контейнер оркестрирует детей с задержкой (stagger),
// каждый ребёнок всплывает снизу. Даёт «оживший» вход экрана вместо разом.
//
// ВАЖНО (Chromium/WebView2): предок с активным filter или opacity<1 образует
// backdrop root — backdrop-filter потомков перестаёт видеть фон страницы, и
// стекло теряет матовость до конца анимации (а остаточный inline `filter:
// blur(0px)` ломал её насовсем). Поэтому:
//   • blur-вход убран вовсе (раньше был filter: blur(4px)→0);
//   • для детей со стеклом есть variant glass: анимируется только transform,
//     фейд панель делает сама — opacity на самом элементе с backdrop-filter
//     матовость не ломает (группа собирается после сэмплинга фона).
const container: Variants = {
  hidden: {},
  show: {
    transition: { staggerChildren: cascade.step, delayChildren: cascade.delay },
  },
};

const item: Variants = {
  hidden: { opacity: 0, y: 14 },
  show: { opacity: 1, y: 0, transition: spring.rise },
};

// Transform-only вариант для стеклянных детей (см. шапку файла).
const itemGlass: Variants = {
  hidden: { y: 14 },
  show: { y: 0, transition: spring.rise },
};

export function Stagger({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <motion.div variants={container} initial="hidden" animate="show" className={className}>
      {children}
    </motion.div>
  );
}

export function StaggerItem({
  children,
  className = "",
  glass = false,
  standalone = false,
}: {
  children: ReactNode;
  className?: string;
  /** Внутри стекло (GlassPanel): не анимируем opacity предка — только сдвиг. */
  glass?: boolean;
  /** Вне контейнера Stagger: элемент запускает свой вход сам (шапки экранов). */
  standalone?: boolean;
}) {
  return (
    <motion.div
      variants={glass ? itemGlass : item}
      {...(standalone ? { initial: "hidden", animate: "show" } : {})}
      // На выходе экрана (AnimatePresence прокидывает exit вглубь) обычные
      // элементы гаснут сами; стеклянные — нет: их фейдит GlassPanel.exit.
      exit={glass ? undefined : { opacity: 0, transition: { duration: dur.fast, ease: ease.exit } }}
      className={className}
    >
      {children}
    </motion.div>
  );
}
