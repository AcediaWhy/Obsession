import { AnimatePresence, motion } from "framer-motion";
import type { ReactNode } from "react";

import { useToastStore, type ToastKind } from "../../store/toastStore";
import { dur, ease, spring } from "../tokens";
import { Icon } from "./icons";

// Контейнер тостов поверх всего (top-right). Стиль — стеклянная пилюля с цветной
// кромкой по типу сообщения. pointer-events только на самих тостах.
const TONE: Record<ToastKind, { ring: string; text: string; icon: ReactNode }> = {
  success: { ring: "border-ok/40", text: "text-ok", icon: <Icon.Check size={16} /> },
  error: { ring: "border-danger/40", text: "text-danger", icon: <Icon.Alert size={16} /> },
  warn: { ring: "border-warn/40", text: "text-warn", icon: <Icon.Alert size={16} /> },
  info: { ring: "border-accent/40", text: "text-accent", icon: <Icon.Info size={16} /> },
};

export function Toaster() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);
  const pause = useToastStore((s) => s.pause);
  const resume = useToastStore((s) => s.resume);

  return (
    <div className="pointer-events-none fixed right-4 top-12 z-50 flex w-80 flex-col gap-2">
      <AnimatePresence initial={false}>
        {toasts.map((t) => {
          const tone = TONE[t.kind];
          return (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, x: 24, scale: 0.98 }}
              animate={{ opacity: 1, x: 0, scale: 1 }}
              exit={{
                opacity: 0,
                x: 24,
                scale: 0.98,
                transition: { duration: dur.fast, ease: ease.exit },
              }}
              transition={spring.snappy}
              // Пока курсор над тостом — автоскрытие стоит: его читают.
              onMouseEnter={() => pause(t.id)}
              onMouseLeave={() => resume(t.id)}
              className={[
                "glass pointer-events-auto flex items-start gap-2.5 rounded-xl border px-3.5 py-2.5 shadow-glass",
                tone.ring,
              ].join(" ")}
            >
              <span className={`mt-0.5 shrink-0 ${tone.text}`}>{tone.icon}</span>
              <p className="flex-1 text-xs leading-relaxed text-ink">{t.message}</p>
              <button
                onClick={() => dismiss(t.id)}
                className="no-drag -mr-1 -mt-0.5 shrink-0 rounded-md p-1 text-ink-muted transition-colors hover:text-ink"
                aria-label="Закрыть"
              >
                <Icon.X size={13} />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
