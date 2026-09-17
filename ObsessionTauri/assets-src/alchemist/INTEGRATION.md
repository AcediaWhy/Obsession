# Alchemist theme

The production theme replaces the Ophanim slot, retaining `ophanim` in localStorage
and `data-theme` for compatibility. Its visible label is `Alchemist`.

- `AlchemistSprite` is shared by the lab and the production core; exact-v4 art and
  the existing pose/potion renderers are unchanged.
- `AlchemistRoom` is shared by the lab and production field; the room, candle,
  bottles, moth, fireflies, three flower cutouts and book ribbon stay in one 3:2 plane.
- `AlchemistCore` maps idle → rest, active → ready, busy/scanning → brew, alarm → rest
  plus a warning marker. Protection actions and disabled states remain app-owned.
- `AlchemistField` covers the window without distorting the artwork. The background
  has no second cat: DPI and Telegram provide the foreground core. Overview keeps
  its standard compact eye/status card, without a character.
- Reduced motion and explicit pause stop animation. Hidden scenes release their
  sprite/room resources. The lab remains available for visual regression checks.

Runtime images use the existing lossless WebP assets in
`public/lab-assets/alchemist-cat/exact-v4`, `scene-v1`, and `scene-v2-flora`.
Original PNG sources remain under `assets-src`; no source artwork is overwritten.
Other experimental artwork and old Ophanim source files are intentionally retained.
