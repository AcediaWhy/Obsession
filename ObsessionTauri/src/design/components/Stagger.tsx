import { motion, type Variants } from "framer-motion";
import type { ReactNode } from "react";

import { cascade, dur, ease, spring } from "../tokens";

// Контейнер запускает появление дочерних элементов с заданным интервалом.
// В Chromium/WebView2 filter или opacity < 1 на предке ограничивают область
// backdrop-filter. Поэтому обёртка стеклянной панели не меняет прозрачность
// и не применяет filter, включая blur(0px).
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

// Прозрачность меняет сама GlassPanel, движение задаёт обёртка экрана.
// Пустые варианты исключают дополнительный сдвиг панели.
const itemGlass: Variants = {
  hidden: {},
  show: {},
};

// Движение заголовка задаёт screenVariants; здесь меняется только прозрачность.
const itemStandalone: Variants = {
  hidden: { opacity: 0 },
  show: { opacity: 1, transition: { duration: dur.base, ease: ease.enter } },
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
  /** Для GlassPanel: обёртка не меняет opacity и не добавляет сдвиг при входе. */
  glass?: boolean;
  /** Вне контейнера Stagger: элемент запускает свой вход сам (шапки экранов). */
  standalone?: boolean;
}) {
  return (
    <motion.div
      variants={standalone ? itemStandalone : glass ? itemGlass : item}
      {...(standalone ? { initial: "hidden", animate: "show" } : {})}
      // Прозрачностью стеклянной панели при выходе управляет GlassPanel.exit.
      exit={glass ? undefined : { opacity: 0, transition: { duration: dur.fast, ease: ease.exit } }}
      className={className}
    >
      {children}
    </motion.div>
  );
}
