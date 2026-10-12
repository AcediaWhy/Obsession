import type { Variants } from "framer-motion";

import { FOCUS_EXIT } from "./focusTransition";

// A paint-free sentinel retains the departing route for its brief content exit.
// Glass fades on its own surface; the route never moves or changes opacity.
export const screenVariants: Variants = {
  enter: { "--route-presence": 1, pointerEvents: "auto" },
  // После прерванного выхода возвращаем странице обработку указателя.
  center: {
    "--route-presence": 1,
    pointerEvents: "auto",
    transition: { duration: 0 },
  },
  exit: {
    "--route-presence": 0,
    // Невидимый уходящий экран остаётся смонтированным и иначе перехватывает клики.
    pointerEvents: "none",
    transition: { duration: FOCUS_EXIT / 1000 },
  },
};
