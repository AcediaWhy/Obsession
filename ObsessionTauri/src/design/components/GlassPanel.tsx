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
  depth?: number; // сила параллакса; 0 по умолчанию отключает смещение
  children?: ReactNode;
};

// Стеклянная панель с подсветкой под курсором. Параллакс по умолчанию отключён:
// смещение может обрезать скруглённые края панели у границ окна.
// При scroll=true прокручивается внутренняя обёртка. Подсветка остаётся
// на неподвижной оболочке, в той же системе координат, что и --mx/--my.
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
    const rect = event.currentTarget.getBoundingClientRect();
    rectRef.current = rect;
    // Первый hover-кадр должен появиться уже под курсором, а не в старой/default
    // точке до следующего тика общего pointer bus.
    event.currentTarget.style.setProperty("--mx", `${event.clientX - rect.left}px`);
    event.currentTarget.style.setProperty("--my", `${event.clientY - rect.top}px`);
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
      data-choir-lens
      // Анимируем прозрачность без scale, чтобы избежать дрожания размытого
      // фона при масштабировании. Движение при входе задаёт обёртка экрана.
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      // Прозрачность меняется на самой панели: opacity на предке ограничивает
      // область backdrop-filter. Состояние exit приходит от AnimatePresence в App.
      exit={{ opacity: 0, transition: { duration: dur.fast, ease: ease.exit } }}
      transition={spring.soft}
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      style={{ x, y, ...style }}
      className={[
        // CSS задаёт плавное изменение тени при переключении glow.
        // Framer Motion не анимирует box-shadow этого элемента.
        "theme-morph glass rounded-xl2 shadow-glass transition-shadow duration-[var(--motion-base)]",
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
