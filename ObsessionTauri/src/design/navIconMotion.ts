import { useEffect, useRef, type AnimationEvent, type FocusEvent, type PointerEvent } from "react";

// Let a finite reaction settle even if the pointer leaves the row. A second
// entry during playback shares that reaction instead of snapping it to frame 0.
export function useNavIconMotion(enabled: boolean, theme: string) {
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const rail = ref.current;
    return () => {
      rail?.querySelectorAll<HTMLElement>("[data-icon-reacting]").forEach((item) => {
        delete item.dataset.iconReacting;
      });
    };
  }, [enabled, theme]);

  function reactToIcon(event: PointerEvent<HTMLElement> | FocusEvent<HTMLElement>) {
    if (!enabled || !(event.target instanceof Element)) return;
    const item = event.target.closest<HTMLButtonElement>(".nav-item");
    if (!item || !event.currentTarget.contains(item)) return;
    if (event.relatedTarget instanceof Node && item.contains(event.relatedTarget)) return;
    item.dataset.iconReacting = "true";
  }

  function finishReaction(event: AnimationEvent<HTMLElement>) {
    // Detail animations can end earlier; only the outer group owns playback.
    if (event.animationName !== "nav-icon-settle" || !(event.target instanceof Element)) return;
    const item = event.target.closest<HTMLElement>(".nav-item");
    if (item) delete item.dataset.iconReacting;
  }

  return { ref, onPointerOver: reactToIcon, onFocus: reactToIcon, onAnimationEnd: finishReaction };
}
