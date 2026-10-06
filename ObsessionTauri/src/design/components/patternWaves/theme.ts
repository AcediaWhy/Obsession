import type { PatternOptions } from "./scene";

export type PatternPalette = Required<Pick<PatternOptions, "color" | "background">>;

export function readPatternPalette(container: HTMLElement): PatternPalette {
  const style = getComputedStyle(container);
  const rgb = (value: string, fallback: number[]) => {
    const channels = value.trim().split(/\s+/).map(Number);
    return channels.length === 3 && channels.every(value => Number.isFinite(value) && value >= 0 && value <= 255)
      ? channels : fallback;
  };
  const accent = rgb(style.getPropertyValue("--z2-accent-rgb") || style.getPropertyValue("--accent"), [99,102,241]);
  const background = rgb(style.getPropertyValue("--z2-scene-rgb"), [13,16,27]);
  return {
    color: [...accent.map(value => value / 255), 1],
    background: [...background.map(value => value / 255), 1],
  };
}

export function observePatternPalette(container: HTMLElement, update: (palette: PatternPalette) => void) {
  let current = readPatternPalette(container);
  update(current);
  const observer = new MutationObserver(() => {
    const next = readPatternPalette(container);
    if (next.color.every((value, index) => value === current.color[index])
      && next.background.every((value, index) => value === current.background[index])) return;
    current = next;
    update(next);
  });
  // Theme attributes can live on the app root or on the document. Watching only
  // this attribute avoids work from pointer styles and animation class changes.
  for (let element: HTMLElement | null = container; element; element = element.parentElement) {
    observer.observe(element, { attributes: true, attributeFilter: ["data-theme"] });
  }
  return () => observer.disconnect();
}
