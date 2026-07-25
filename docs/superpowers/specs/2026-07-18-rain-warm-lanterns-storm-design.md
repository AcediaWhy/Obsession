# Rain Theme: Warm Lanterns and Dramatic Storm Design

> **SUPERSEDED** (2026-07-18): тема перестроена в «окно ночного поезда» — см. 2026-07-18-night-train-realism-webgl2-design.md. Ночная сакура/фонари удалены из кода.

## Problem

The Rain theme (`japan`) renders the night-sakura photo with an even cold
grade: the paper lanterns — the brightest spots of the photo — read cyan
instead of amber, so the scene loses its warm-light focal points. Separately,
the "storm" reaction to an active bypass flips two rain parameters instantly
(`rainChance 0.3→0.5`, `rainLimit 10→16`), which reads as a barely noticeable
step change rather than weather building up.

## Scope

Limited to the WebGL rain scene:

- `src/design/components/rain/shaders.ts` — color grading in `water.frag`
- `src/design/components/rain/rainRenderer.ts` — new uniforms
- `src/design/components/RainScene3D.tsx` — storm factor animation, drop
  parameters, warmth uniform feed

The background photo (`public/rain/bg.jpg`) stays as is. `RainCore`,
`RainField2D`, other themes, and the simulation core (`raindrops.ts`) are
unchanged except where the storm factor feeds existing `Raindrops` options.

## Design

### Warm lantern color grade (shader)

`water.frag` gains a split-tone grade applied to both the frosted background
sample (`bg`) and the refracted drop content (`tex` before blending), so drops
act as lenses of the same graded scene:

- Luminance of the sample keys a warm tint: highlights converge toward a
  fixed amber hue (`vec3(1.0, 0.55, 0.24) * luma * 1.3`) — a multiplicative
  tint was rejected because it keeps the blown-out cyan channel ratios;
  shadows get a light cold bias (`color * vec3(0.92, 1.0, 1.12)`).
- The grade mixes cold → amber via `smoothstep(0.30, 0.70, luma)`: the night
  sky and sakura (luma ≲ 0.38) stay cold, only genuinely bright areas
  (lantern glass, luma ≳ 0.7) ignite.
- A single scalar uniform `u_warmth` (0..1.2) scales the effect. The grade
  function is pure GLSL ALU — no extra textures, no CPU pixel passes.

`RainRenderer` registers `u_warmth` (default 1.0) and exposes a `warmth`
property written each frame from `draw()`, alongside the existing parallax
uniform update.

### Storm factor

`RainScene3D` replaces the instant parameter flip with a smoothed storm factor:

- `storm` lerps toward `1` (bypass/proxy active) or `0` (idle) with an
  exponential approach (≈1.5 s time constant), seeded from current state at
  mount so a hot start shows full storm immediately — same pattern as `warm`
  in `RainCore`.
- Rain parameters derive from `storm` each frame:
  - `rainChance`: 0.3 → 0.7
  - `rainLimit`: 10 → 26
  - `globalTimeScale`: 0.45 → 0.75 (drops crawl visibly faster)
  - `dropletsRate`: 0 → 30 (library default is 50; 30 gives fine spray
    without crowding the glass)
- Lantern warmth breathes with the storm: `u_warmth = 1.0 + storm * (0.15 +
  0.05 * sin(t))` — a subtle slow pulse, capped well below visible flicker.

### Error handling and fallbacks

- If WebGL/asset load fails, the existing `RainField2D` fallback shows; the
  2D fallback keeps its current look (no warm grade) — out of scope.
- New uniforms have safe defaults, so a stale renderer instance cannot render
  black if the scene forgets to feed them.

## Verification

- `npm run build` (TypeScript + Vite production build) passes.
- Rain theme shows lanterns glowing warm amber against the cold night in both
  the frosted background and inside drop lenses.
- Toggling DPI/proxy on: rain visibly builds over ~1.5 s into a heavy
  downpour with faster drops; toggling off settles back smoothly.
- No console warnings; frame rate stays on par with the current scene at
  high quality tier.
- Existing `rainFramePipeline` and renderer tests still pass.
