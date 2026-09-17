# User-cleaned alchemist layers

Unmodified copies from Downloads (2026-09-15):

- body.png: ChatGPT Image Sep 15, 2026, 03_34_20 PM.png — 1241 × 1267.
- flask.png: ChatGPT Image Sep 15, 2026, 03_35_40 PM.png — 1254 × 1254.
- eyes-open.png: ChatGPT Image Sep 15, 2026, 03_37_45 PM (2).png.
- eyes-half.png: ChatGPT Image Sep 15, 2026, 03_37_45 PM (3).png.
- eyes-closed.png: ChatGPT Image Sep 15, 2026, 03_37_45 PM (1).png.

All have transparent backgrounds. Closed-eye black marks are present, despite
being invisible against a black image viewer background.

Runtime source rectangles and placements: src/labs/alchemistLayers.ts. The body
is proportionally contained in the square stage. Each eye is individually aligned
to consistent width and lower-lid anchors. Flask/paws are repositioned over the
belly. Files themselves are neither cropped nor resized. No new generation.

The current lab uses a 14-second discrete ritual timeline (alchemistMotion.ts):
rest, inspect with half-lidded eyes, raise flask/paws in four 150ms steps, a stronger
potion reaction, blink, lower in four steps, and a long rest. The lift is 24 source
units (8 physical pixels at 440px, 2 at 104px). Only brew mode performs the ritual;
rest and ready retain an occasional blink. The eye selector overrides eye poses.

Canvas redraws only when the pose or resolution changes, with image smoothing
disabled. The main body and hat remain stationary; local tail/ear-tip gestures
are described below. The flask's position is rounded to
physical pixels independently from its fixed dimensions, so movement cannot
alter the flask scale. Native glow and bubbles follow the same snapped offset.
No raster rotation, fractional movement or opacity crossfade is used.

Tail and ear twitches (alchemistTwitch.ts) use small bounded regions of the
already pixel-snapped body canvas. Rows shift by whole physical pixels, tapering
to zero at the bottom join. This is a subtle runtime deformation of the existing
art, not newly generated layers or a fully articulated rig. Original PNGs remain
unchanged. The tail twitches at 1.8s and 7.9s; the ears react at 3.4s and 3.6s,
with short held poses and long rests. All three moods retain these gestures.
Reduced motion disables them; pause and hidden-page handling use the same clock
as the flask ritual. Tests cover integer offsets, anchored joins and timing.

Pause freezes the clock and CSS effects; hidden pages stop requestAnimationFrame,
and reduced motion displays a stationary pose. “Показать действие” restarts all
three previews at the inspect pose and resumes playback (unless reduced motion
is enabled). Sprite lifecycle cleanup releases observers, events and animation
requests. Source images remain unchanged.

Only the standalone alchemist lab uses these layers. The single-PNG illustration
is retained as fallback if loading any layer fails. Potion glow/bubbles remain
native overlays; liquid is still part of the flask PNG, not a separate liquid mesh.
