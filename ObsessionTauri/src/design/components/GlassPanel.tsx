import { motion, type HTMLMotionProps } from "framer-motion";
import type { ReactNode } from "react";
import { spring } from "../tokens";
import { useParallaxOffset } from "../parallax";

type Props = Omit<HTMLMotionProps<"div">, "children"> & {
  padded?: boolean;
  glow?: boolean;
  spotlight?: boolean;
  depth?: number; // сила параллакса (средний план по умолчанию)
  children?: ReactNode;
};

// Базовая стеклянная панель Singularity: specular-кромка + прожектор за курсором
// + spotlight-кант + параллакс среднего плана (панель отделяется от фона по
// глубине при движении мыши).
export function GlassPanel({
  padded = true,
  glow = false,
  spotlight = true,
  depth = 6,
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
        padded ? "p-5" : "",
        className,
      ].join(" ")}
      {...rest}
    >
      {spotlight && <span className="spotlight-ring" aria-hidden />}
      {children}
    </motion.div>
  );
}
