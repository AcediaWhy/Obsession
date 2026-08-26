import type { YaniPanelLens } from "./types";

type RectLike = { left: number; top: number; width: number; height: number; radius?: number };

export function normalizeYaniPanelLenses(
  panels: readonly RectLike[],
  viewport: RectLike,
  limit = 12,
): YaniPanelLens[] {
  if (viewport.width <= 0 || viewport.height <= 0 || limit <= 0) return [];
  return panels
    .filter((panel) => panel.width > 0 && panel.height > 0)
    .slice(0, Math.min(12, Math.floor(limit)))
    .map((panel) => ({
      x: (panel.left - viewport.left) / viewport.width,
      y: 1 - (panel.top - viewport.top + panel.height) / viewport.height,
      width: panel.width / viewport.width,
      height: panel.height / viewport.height,
      radius: Math.max(0, panel.radius ?? 0) / Math.min(viewport.width, viewport.height),
    }));
}
export function readYaniPanelLenses(viewport: DOMRect, limit = 12): YaniPanelLens[] {
  const panels = [...document.querySelectorAll<HTMLElement>(".glass")].map((element) => {
    const rect = element.getBoundingClientRect();
    const radius = Number.parseFloat(getComputedStyle(element).borderTopLeftRadius) || 0;
    return { left: rect.left, top: rect.top, width: rect.width, height: rect.height, radius };
  });
  return normalizeYaniPanelLenses(
    panels,
    { left: viewport.left, top: viewport.top, width: viewport.width, height: viewport.height },
    limit,
  );
}
