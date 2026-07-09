import { motion, type HTMLMotionProps } from "framer-motion";
import type { ReactNode } from "react";
import { spring } from "../tokens";
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
  // прокрутки → псевдоэлементы не уезжают).
  const inner = scroll ? (
    <div
      className={["h-full overflow-y-auto", padded ? "p-5" : "", contentClassName].join(" ")}
    >
      {children}
    </div>
  ) : (
    children
  );

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.985 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={spring.soft}
      onMouseMove={onMove}
      style={{ x, y, ...style }}
      className={[
        "glass snow-surface rounded-xl2 shadow-glass",
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
