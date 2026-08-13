import type { ReactNode } from "react";
import { motion } from "framer-motion";

type CoreShellProps = {
  children: ReactNode;
  interactive?: boolean;
  busy: boolean;
  onClick: () => void;
  size: number;
};

// Один и тот же renderer нужен и для hero-кнопки, и для декоративного превью.
// Во втором случае нельзя оставлять <button> внутри плитки выбора темы: это
// нарушает семантику и ломает клавиатурную навигацию. Сам canvas остаётся — он
// рисует оригинальную айдентику, но с paused={true} делает только стоп-кадр.
export function CoreShell({
  children,
  interactive = true,
  busy,
  onClick,
  size,
}: CoreShellProps) {
  const className = "no-drag relative flex items-center justify-center disabled:cursor-wait";
  const style = { width: size, height: size };

  if (!interactive) {
    return (
      <div aria-hidden="true" className={className} style={style}>
        {children}
      </div>
    );
  }

  return (
    <motion.button
      type="button"
      onClick={onClick}
      disabled={busy}
      whileHover={{ scale: 1.03 }}
      whileTap={{ scale: 0.97 }}
      className={className}
      style={style}
    >
      {children}
    </motion.button>
  );
}
