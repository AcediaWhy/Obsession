import type { ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";

import { useSettingsStore } from "../../store/settingsStore";
import { dur, ease, spring } from "../tokens";

export function Collapse({
  open,
  children,
  className = "",
}: {
  open: boolean;
  children: ReactNode;
  className?: string;
}) {
  const systemMotionOff = useReducedMotion();
  const settingsMotionOff = useSettingsStore(
    (state) => state.settings?.reduce_motion ?? false,
  );
  const motionOff = Boolean(systemMotionOff || settingsMotionOff);

  return (
    <AnimatePresence initial={false}>
      {open ? (
        <motion.div
          initial={motionOff ? false : { opacity: 0, height: 0 }}
          animate={{
            opacity: 1,
            height: "auto",
            transition: motionOff
              ? { duration: 0 }
              : {
                  height: spring.expand,
                  opacity: { duration: dur.base, ease: ease.enter },
                },
          }}
          exit={{
            opacity: 0,
            height: 0,
            transition: motionOff
              ? { duration: 0 }
              : {
                  height: { duration: dur.base, ease: ease.exit },
                  opacity: { duration: dur.fast, ease: ease.exit },
                },
          }}
          className={`overflow-hidden ${className}`}
        >
          {children}
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
