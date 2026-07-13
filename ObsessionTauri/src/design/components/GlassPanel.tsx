import { motion, type HTMLMotionProps } from "framer-motion";
import type { ReactNode } from "react";
import { dur, ease, spring } from "../tokens";
import { useParallaxOffset } from "../parallax";

type Props = Omit<HTMLMotionProps<"div">, "children"> & {
  padded?: boolean;
  glow?: boolean;
  spotlight?: boolean;
  scroll?: boolean; // прокрутка контента ВНУТРИ панели (см. ниже)
  contentClassName?: string; // классы для внутренней скролл-обёртки при scroll
  depth?: number; // сила параллакса (средний план по умолчанию)
  children?: ReactNode;
};

// Базовая стеклянная панель: specular-кромка + прожектор за курсором +
// spotlight-кант. Параллакс по умолчанию ВЫКЛЮЧЕН (depth 0): при сдвиге панели
// её скруглённые верх/бок уезжали под рамку окна и «отгрызались» краем. Глубину
// теперь даёт только фон (HeroField с оверсканом); панели прибиты к макету.
//
// scroll=true: прокрутка уходит во ВНУТРЕННЮЮ обёртку, а сама оболочка остаётся
// нескроллящейся. Это критично для spotlight: псевдоэлементы `.spotlight::after`
// и `.spotlight-ring` спозиционированы `absolute; inset:0` относительно оболочки.
// Если бы скроллилась сама оболочка (overflow на .glass), они бы уезжали вместе с
// контентом, а `--mx/--my` (координаты видимой рамки) — нет: кольцо «отрывалось»
// и застывало рамкой у края, переставая следовать за курсором.
export function GlassPanel({
  padded = true,
  glow = false,
  spotlight = true,
  scroll = false,
  contentClassName = "",
  depth = 0,
  className = "",
  style,
  children,
  ...rest
}: Props) {
  const { x, y } = useParallaxOffset(depth);

  const onMove = spotlight
    ? (e: React.MouseEvent<HTMLDivElement>) => {
        const r = e.currentTarget.getBoundingClientRect();
        e.currentTarget.style.setProperty("--mx", `${e.clientX - r.left}px`);
        e.currentTarget.style.setProperty("--my", `${e.clientY - r.top}px`);
      }
    : undefined;

  // При scroll: паддинг и layout-классы контента переносим на внутреннюю
  // обёртку; оболочка получает overflow-hidden (клип по скруглению, без своей
  // прокрутки → псевдоэлементы не уезжают). scroll-fade растворяет строки у
  // кромок вместо жёсткого среза (маска на обёртке — specular не страдает).
  const inner = scroll ? (
    <div
      className={["scroll-fade h-full overflow-y-auto", padded ? "p-5" : "", contentClassName].join(" ")}
    >
      {children}
    </div>
  ) : (
    children
  );

  return (
    <motion.div
      // Вход/выход — ЧИСТЫЙ fade, без scale: масштаб элемента с backdrop-filter
      // заставляет композитор пересэмплировать блюр каждый кадр (регион выборки
      // меняет геометрию) и даёт «плывущее» дрожание заблюренного фона. Движение
      // входа панель получает бесплатно от обёртки экрана (слайд в App) и
      // StaggerItem (y-подъём) — оба transform-only снаружи стекла.
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      // Панель гаснет САМА (own-opacity не ломает свой backdrop-filter), когда
      // экран уходит: AnimatePresence в App прокидывает exit вглубь дерева.
      // Обёртки экрана/StaggerItem при этом остаются transform-only.
      exit={{ opacity: 0, transition: { duration: dur.fast, ease: ease.exit } }}
      transition={spring.soft}
      onMouseMove={onMove}
      style={{ x, y, ...style }}
      className={[
        // transition-shadow: тумблер glow (командный центр Обзора при включении
        // защиты) расцветает за 0.5с, а не щёлкает. Framer box-shadow здесь не
        // анимирует — конфликта нет.
        "glass rounded-xl2 shadow-glass transition-shadow duration-500",
        spotlight ? "spotlight" : "",
        glow ? "shadow-glow" : "",
        scroll ? "overflow-hidden" : padded ? "p-5" : "",
        className,
      ].join(" ")}
      {...rest}
    >
      {spotlight && <span className="spotlight-ring" aria-hidden />}
      {inner}
    </motion.div>
  );
}
