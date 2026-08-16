# Obsession — The Fixation

## Role

Obsession is the public flagship theme and the visual identity of the app. Its
composition is built around a single fixation point: an abstract optical eye
that behaves as a lens, aperture and targeting mark at the same time. It is not
a biological eye and does not replace the global app logo.

The first impression should feel controlled and expensive. The unease appears
only after looking longer: threads keep converging, glass stress catches the
light and the lens reacts to runtime state without becoming a dashboard gauge.

## Visual language

- Base: charcoal black and black optical glass.
- Interface light: pearl and muted silver.
- Internal energy: deep carmine, restricted to lenses, caustics and focus
  threads. Bright red remains reserved for real application errors.
- Geometry: the existing screen layout is unchanged. Theme tokens,
  pseudo-elements and narrowly scoped `data-theme="obsession"` rules create
  the skin.
- Motion: nearly still at rest, with rare decisive optical events. The focus
  point remains in the upper-right negative space and moves softly between
  screen-specific positions. Pointer input adds parallax only; it never steers
  the focal point.

## Runtime projection

The frontend-only `ObsessionVisualPhase` projection aggregates existing stores
without sending data to the backend:

1. `fault`: explicit runtime errors, blind/process-failed recovery, terminal
   halt or exhausted search.
2. `scanning`: tests, strategy search/application/rollback, confirmation,
   recovery or Brain switching.
3. `engaging`: DPI or proxy start/stop transition.
4. `focused`: DPI or proxy is active.
5. `idle`: no relevant activity.

The priority is intentional: concurrent faults must remain visible even while
another subsystem is scanning or active.

| Phase | Optical response |
|---|---|
| `idle` | Threads remain slightly soft; movement is almost imperceptible. |
| `engaging` | The aperture closes and threads gather sharply. |
| `scanning` | The focus traverses a small invisible grid while the lens refocuses. |
| `focused` | The mark stabilizes; internal carmine becomes deep and even. |
| `fault` | A short split focus creates optical diplopia without continuous flashing. |

## Rendering pipeline

The full scene is a single-pass Hybrid WebGL2 renderer. At construction time it
generates deterministic low-resolution optical color and normal plates, uploads
them once, then combines them with procedural refraction, lens stress, caustics,
chromatic edge treatment, converging threads and phase-specific distortion.
There is no animated random grain and no network or media dependency.

Quality policy:

- `high`: full resolution, 60 FPS, 16 threads, caustics and full aberration;
- `balanced`: 0.72 resolution, 30 FPS, 10 threads and reduced aberration;
- `low`: 0.52 resolution, 30 FPS, 6 threads, no secondary caustic or aberration
  pass.

The shared frame scheduler, pointer bus and visibility/reduced-motion gates are
used. Hidden windows unmount the field and release its GPU resources. Reduced
motion and an initially paused scene both draw one expressive stop-frame.

## Fallback and hero

The SVG/CSS fallback reproduces the same upper-right fixation point, threads,
optical mark and pearl/carmine palette. It is shown while WebGL compiles and
becomes permanent when WebGL2 or shader initialization fails.

`ObsessionCore` turns the hero control into a compact objective. The eye-sign is
assembled by the aperture rather than printed on top. It implements the shared
`active`, `busy`, `scanning`, `alarm`, `paused` and `interactive` contract. The
theme preview uses the same renderer with `interactive={false}`, so theme tiles
never contain nested buttons.

## Entry transition and accessibility

Switching from another theme triggers a 650 ms Focus Capture overlay: the old
scene darkens toward the screen-specific focal point and the new optical field
opens from it. A cold start uses the normal crossfade, and reduced motion skips
the capture completely. The overlay is decorative and never receives input.

Pearl remains the normal focus/selection color. Carmine is not the only state
signal: existing success, warning and error colors, text and shapes retain
their semantic meaning. Keyboard focus remains explicit and no screen behavior
changes with the theme.
