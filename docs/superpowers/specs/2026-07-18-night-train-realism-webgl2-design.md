# Night Train Realism (WebGL2) Design

Supersedes the water/world parts of `2026-07-18-rain-warm-lanterns-storm-design.md`
(its warm-lantern grade and storm factor live on in spirit inside the new
composite shader and weather coupling). Reference implementations studied:
SardineFish `raindrop-fx` (CPU drop physics, mist, mip-blur) and BigWings
`Heartfelt` (per-pixel mip-LOD focus, fog cut by trails). Reference photos:
dark coupe interior as rim-lit silhouettes, misted glass with wiped trails,
sharp world inside drop lenses, speed-blurred green forest, photo-4
composition (window left, bunks right, table below).

## Problem

The current hybrid train scene is a single-pass WebGL1 sketch: no per-pixel
focus (the #1 realism cue in both references — needs mip levels, unavailable
for NPOT textures in WebGL1), drops without mass/trail physics, no
condensation layer, and a centered window that reads as a vignette rather
than a train interior.

## Scope

Replace the hybrid renderer with a WebGL2 two-pass pipeline; upgrade the CPU
simulations (drops with mass, condensation); new interior composition. Keep:
`weather.ts`, quality-tier system, `frameScheduler`, `RainTrainFallback`,
`RainLanternCore` (hero core, separate component), the `rain-dev.html`
harness (same component API). Delete the superseded old stack:
`RainScene3D.tsx`, `RainField2D.tsx`, `RainCore.tsx`, `rain/raindrops.ts`,
`rain/rainRenderer.ts`, `rain/shaders.ts`, `rain/gl.ts`, `rain/webgl.ts`,
`rain/random.ts`, `rain/simulation.ts`, `rain/dropModel.ts`,
`rain/dropModel.test.ts`, `rain/hybridShaders.ts`,
`rain/hybridRenderer.ts`, and `/public/rain/*` assets used only by it.

## Design

### Pipeline (GPU, WebGL2)

```
CPU per frame: weather → dropSim → water map (normals+thickness+alpha)
                        mistSim → mist map (condensation density)
GPU per frame:
  Pass A (world): fullscreen procedural night world (forest/pines/field/
                  sky/drizzle/lightning, speed streaks) → RGBA8 FBO;
                  generateMipmap each frame (LINEAR_MIPMAP_LINEAR)
  Pass B (composite): fullscreen; samples world via textureLod with
                  per-pixel focus (sharp inside drop lenses, blurred on
                  misted glass); interior silhouettes + sprites outside the
                  window SDF; grade + vignette + grain
```

WebGL2 context creation failure, or context loss that does not restore,
falls back to `RainTrainFallback` through the existing error-boundary path.

### Water (hero)

- **Drop sim v2 (CPU, raindrop-fx style):** each drop has mass/radius and a
  motion state (stationary/sliding, re-rolled on a per-drop interval);
  gravity acceleration (~2400 px/s² scaled); wind/x-shift from train speed
  via the existing weather snapshot.
- **Trail deposit:** a sliding drop spawns small static trail drops
  (0.3–0.5× radius) and loses mass accordingly (`trailDropDensity`); trail
  drops sit still, evaporate and shrink away. Trails therefore exist where
  drops actually went — prerequisite for honest mist wiping.
- **Merging:** existing spatial hash, area-conserving; evaporation removes
  sub-threshold drops.
- **Water map:** 64px normal/thickness/alpha sprite stamps (as now) with
  mass-proportional alpha, uploaded per frame.
- **Composite:** `focus = mix(maxBlurLod, minBlurLod, dropMask)`;
  `textureLod(world, uv + normal*refraction, focus)`. Drops catch sky
  brightness (brighter than dark glass) plus a specular glint from a fixed
  light direction — the volume cue visible in the reference photos.

### Condensation (mistSim, new)

- Coarse density grid over the window region; density grows toward a cap
  over `mistTime`; drop trails subtract density (eraser radius ∝ drop
  radius); wiped cells regrow slowly.
- Storm coupling: faster condensation; heavy drops wipe more.
- Rendered to a single-channel mist map; in the composite it raises LOD and
  adds a faint milky veil. Micro-droplet sparkle is procedural (static
  drops field) gated by mist density — no per-droplet sprites needed.

### World (night)

- Night palette (dark blue-green), rare distant lights; pine/trunk layer in
  fog (photo 3), field, layered depth; analytic speed-streak blur; drizzle
  veil; lightning wash (existing weather model supplies the pulse).
- Mips are consumed only by the composite (DOF/focus) — the world pass
  itself stays sharp.

### Interior (photo-4 composition)

- Window SDF on the **left** (~62% of frame width), rounded corners, seal
  shadow, frame mist at the edges.
- Right side: **upper bunk** (mattress + rolled blankets) and **lower
  berth** with a pillow — procedural SDF silhouettes with noise folds.
- **Table** at the bottom with small items (mug, bottle, phone) from a
  **runtime-drawn sprite atlas**: a Canvas2D atlas painted by our own code
  at init (no binary assets, no photos), uploaded as a texture and sampled
  at hardcoded placements.
- Lighting: rim light from the window by SDF distance (cool key), faint
  warm coupe fill; interior stays ~90% silhouette as in the photos.

### Performance and quality tiers

- FBO sized to the GL backing (css × quality scale); mip generation is one
  call per frame and cheap at these sizes.
- Tiers control: backing scale, usable mip depth, drop cap, mist grid
  resolution, interior fold detail. Budget: high tier within ~1.5 ms of the
  current scene; the existing frameScheduler degrades tiers automatically.
- Reduced motion renders a single coherent frame (existing scheduler
  behavior; weather snaps, lightning zeroed).

### Module layout

```
rain/gl2.ts              WebGL2 context/program/FBO helpers
rain/pipeline.ts         pass orchestration, textures, resize/destroy
rain/worldShaders.ts     pass A vertex/fragment
rain/compositeShaders.ts pass B vertex/fragment
rain/dropSim.ts          drop sim v2 (replaces dropModel+simulation)
rain/mistSim.ts          condensation model
rain/waterMap.ts         sprite stamping into normal/thickness canvas
rain/interiorAtlas.ts    runtime Canvas2D sprite atlas
RainHybridScene.tsx      rewired phases (same component API)
rain/quality.ts          extended profile (world scale, mip depth, caps)
```

`hybridFramePipeline.ts` phases extend to: input → weather → simulate →
upload → worldPass → compositePass.

### Migration

Old stack files (listed in Scope) are deleted in the same change; nothing
imports them after `HeroField` switches to the rewired scene. The harness
keeps `?active=1` and `?warp=1` working against the new pipeline.

## Verification

- `npm run build` and the full vitest suite pass.
- New unit tests (node env, seeded RNG): dropSim mass accounting
  (merge + trail deposit conserve mass minus evaporation), trail deposit
  shrinks the parent, evaporation removes static drops, cap trim keeps
  largest; mistSim growth/wipe/regrow; pipeline phase order.
- Harness (`rain-dev.html`): idle shows misted night window with sparse
  drops; `?active=1` builds a storm — train accelerates, drops streak
  diagonally, mist wipes in trails; drops show a sharp refracted world
  while misted glass stays blurred (mip-LOD focus).
- Headless screenshot check (SwiftShader + `?warp=1`) confirms both passes
  render and the water map reaches the composite.
- Old-stack files are gone; no dangling imports.
