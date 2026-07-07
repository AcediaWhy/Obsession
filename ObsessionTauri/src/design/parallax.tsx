import { createContext, useContext, useEffect, type CSSProperties, type ReactNode } from "react";
import {
  motion,
  useMotionValue,
  useSpring,
  useTransform,
  type MotionValue,
} from "framer-motion";

// Единый источник положения указателя для параллакса. Нормализуем курсор в
// диапазон -1..1 от центра окна и сглаживаем пружиной — слои двигаются плавно,
// без «дёрганья» за мышью.
type Ctx = { px: MotionValue<number>; py: MotionValue<number> };
const ParallaxContext = createContext<Ctx | null>(null);

export function ParallaxProvider({ children }: { children: ReactNode }) {
  const rawX = useMotionValue(0);
  const rawY = useMotionValue(0);
  const cfg = { stiffness: 120, damping: 22, mass: 0.4 };
  const px = useSpring(rawX, cfg);
  const py = useSpring(rawY, cfg);

  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      rawX.set((e.clientX / window.innerWidth - 0.5) * 2);
      rawY.set((e.clientY / window.innerHeight - 0.5) * 2);
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => window.removeEventListener("pointermove", onMove);
  }, [rawX, rawY]);

  return <ParallaxContext.Provider value={{ px, py }}>{children}</ParallaxContext.Provider>;
}

// Смещение в пикселях = нормализованный указатель × depth. Положительный depth —
// слой движется вслед за курсором (ближе к зрителю), отрицательный — против
// (дальше). Хук всегда вызывает одинаковый набор под-хуков, поэтому безопасен
// и вне провайдера (тогда смещение = 0).
export function useParallaxOffset(depth: number) {
  const ctx = useContext(ParallaxContext);
  const zeroX = useMotionValue(0);
  const zeroY = useMotionValue(0);
  const px = ctx?.px ?? zeroX;
  const py = ctx?.py ?? zeroY;
  const x = useTransform(px, (v) => v * depth);
  const y = useTransform(py, (v) => v * depth);
  return { x, y };
}

// Готовый слой-обёртка: сдвигает содержимое по параллаксу.
export function Parallax({
  depth = 10,
  className = "",
  style,
  children,
}: {
  depth?: number;
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}) {
  const { x, y } = useParallaxOffset(depth);
  return (
    <motion.div className={className} style={{ x, y, ...style }}>
      {children}
    </motion.div>
  );
}
