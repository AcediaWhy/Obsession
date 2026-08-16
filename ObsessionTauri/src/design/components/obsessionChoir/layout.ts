export type ChoirFocus = { x: number; y: number };

const SCREEN_FOCUS: Record<string, ChoirFocus> = {
  overview: { x: 0.775, y: 0.66 },
  dpi: { x: 0.78, y: 0.71 },
  ai: { x: 0.755, y: 0.68 },
  telegram: { x: 0.77, y: 0.69 },
  lists: { x: 0.74, y: 0.69 },
  profiles: { x: 0.775, y: 0.67 },
  settings: { x: 0.73, y: 0.71 },
};

export function obsessionChoirFocusForScreen(screen: string): ChoirFocus {
  return SCREEN_FOCUS[screen] ?? SCREEN_FOCUS.overview;
}
