import { motion, type HTMLMotionProps } from "framer-motion";
import { useEffect, useRef, type ReactNode } from "react";
import { dur, ease, spring } from "../tokens";
import { useParallaxOffset } from "../parallax";
import { subscribePointerFrame } from "../pointerBus";

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
  onPointerEnter: onPointerEnterProp,
  onPointerLeave: onPointerLeaveProp,
  ...rest
}: Props) {
  const { x, y } = useParallaxOffset(depth);
  const panelRef = useRef<HTMLDivElement>(null);
  const rectRef = useRef<DOMRect | null>(null);
  const pointerInsideRef = useRef(false);

  useEffect(() => {
    if (!spotlight) return;
    const panel = panelRef.current;
    if (!panel) return;

    const updateRect = () => {
      rectRef.current = panel.getBoundingClientRect();
    };
    updateRect();

    const resizeObserver =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(updateRect);
    resizeObserver?.observe(panel);

    const unsubscribePointer = subscribePointerFrame((pointer) => {
      if (!pointerInsideRef.current) return;
      if (pointer.layoutChanged) updateRect();
      const rect = rectRef.current;
      if (!rect) return;
      panel.style.setProperty("--mx", `${pointer.clientX - rect.left}px`);
      panel.style.setProperty("--my", `${pointer.clientY - rect.top}px`);
    });

    return () => {
      unsubscribePointer();
      resizeObserver?.disconnect();
    };
  }, [spotlight]);

  const handlePointerEnter: NonNullable<Props["onPointerEnter"]> = (event) => {
    pointerInsideRef.current = true;
    rectRef.current = event.currentTarget.getBoundingClientRect();
    onPointerEnterProp?.(event);
  };
  const handlePointerLeave: NonNullable<Props["onPointerLeave"]> = (event) => {
    pointerInsideRef.current = false;
    onPointerLeaveProp?.(event);
  };

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
      ref={panelRef}
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
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      style={{ x, y, ...style }}
      className={[
        // transition-shadow: тумблер glow (командный центр Обзора при включении
        // защиты) расцветает за 0.5с, а не щёлкает. Framer box-shadow здесь не
        // анимирует — конфликта нет.
        "theme-morph glass rounded-xl2 shadow-glass transition-shadow duration-500",
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
