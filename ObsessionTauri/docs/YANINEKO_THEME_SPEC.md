# Yani Neko — Living Mask in Room 03:17

## Role

`yanineko` remains a secret frontend-only theme and keeps its existing ID,
unlock phrases and `localStorage` persistence. It is a procedural homage to
Yaniko Sato from «ヤニねこ»: beautiful, lazy, untidy, nicotine-dependent and
oddly charming. No anime frames, raster character art, audio, backend calls or
new runtime dependencies are included.

The field contains only her room. Yaniko exists exclusively in the interactive
hero core, so glass panels retain calm negative space and never cover a second
face or silhouette.

## Visual language

- Base: tobacco near-black and worn wood instead of blue-black.
- Cool light: mint hair and moonlight (`--accent-cyan: 191 227 207`).
- Warm light: amber eyes and ember (`--accent: 232 137 46`).
- Material accents: paper cream, the pack's muted blue band, minimal inner-ear
  pink and two gold rings on the left ear.
- Glass: warm smoky brown with a mint specular edge, restrained amber pointer
  light and panel-aware optical distortion in the GPU composite.
- Motion: lazy at rest, coherent rather than noisy. Seeded event windows drive
  blinks, saccades, ear twitches and ring glints; there is no `Math.random()` in
  a render frame.

## State director

Priority is `alarm > scanning > busy > active > idle`. Existing application
telemetry maps as follows:

| Shared phase | Yani mood | Core | Room |
|---|---|---|---|
| `idle` | `idle` | half-lidded gaze, breathing, rare twitches | cool beam, thin smoke, weak ember |
| `engaging` | `busy` | short drag, compressed cheeks/lids, ears forward | ember flare, smoke pulled to source |
| `scanning` | `scanning` | wide anxious eyes, independent ears | cross-draft, faster dust and smoke |
| `focused` | `active` | bright relaxed gaze, long exhale | warmer balance and denser smooth plume |
| `fault` | `alarm` | grotesque grimace, pinned ears, tiny pupils | ember almost dies, ash and cold flicker |

`deriveYaniMood` is the single priority function for direct core signals;
`yaniMoodForPhase` is the field projection. Springs use clamped `dt`, so a zero
step, dropped frame or resume cannot generate NaN or teleport an expression.

## Living mask core

`YaniNekoCore` is a high-detail Canvas 2D renderer inside the unchanged square
`CoreShell` hit-area. The old disc, radial mask, circular rim and `ON/OFF` label
are gone. Transparent corners reveal one asymmetric cat-head silhouette with:

- soft jaw and warm face;
- two ears that emerge above the head and react independently;
- layered mint fringe;
- amber slit-pupil eyes;
- two glinting rings on the left ear;
- a diagonal cigarette with ember, ash and smoke.

Hover directs the eyes and nearer ear. A switch into `engaging`/`focused`
creates a connected inhale/exhale sequence. The same renderer supports 240,
220 and 104 px; 104 px is the decorative preview and never creates a nested
button.

## Procedural room renderer

`YaniNekoField` owns a WebGL2 pipeline with an immediate Canvas 2D fallback.
It accepts `phase`, `screen`, `paused` and an optional forced `qualityTier` for
the harness. Screen changes make only a small composition shift; the window and
desk remain recognizable.

The WebGL path has three passes:

1. Ping-pong RGBA8 smoke feedback with curl advection, density, heat, source
   injection above the ashtray and a very weak pointer wake.
2. Procedural world pass: upper-right window and blinds, mint moon shaft,
   worn desk, ashtray, cigarette pack, can, cloth, dust, ember and falling ash.
3. Composite pass: smoke, soft bloom, warm/cool grading, film grain, vignette
   and refraction inside normalized `.glass` rectangles.

`ResizeObserver` keeps backing stores and panel coordinates current;
`MutationObserver` catches screen/panel changes. At most twelve lenses are sent
to the shader. Shader construction failure, missing WebGL2 or context loss
switches to a compositionally matching Canvas renderer. Unmount disconnects
observers/listeners, disposes the shared render loop, deletes programs, textures,
framebuffers, buffer and VAO, then zeros the backing canvas.

The existing `HeroField` render gate remains authoritative: hidden/tray scenes
unmount. Reduced motion and explicit pause produce one composed still frame.

## Adaptive quality

The shared `FrameScheduler` is unchanged. It controls cadence and supplies the
adaptive tier; Yani lowers its own resolution and shader budgets first.

| Tier | Display cap | World | Smoke | Noise | Glass lenses |
|---|---:|---:|---:|---:|---:|
| high | 180 Hz | 0.90 | 0.50 | 5 octaves | 12 |
| balanced | 120 Hz | 0.75 | 0.38 | 4 octaves | 8 |
| low | 60 Hz | 0.60 | 0.28 | 3 octaves | 6 |

Simulation time is based on clamped real `dt`, not frame count, so degradation
does not slow the room down.

## Navigation materials

The real `NavRail` button semantics and layout are unchanged. A Yani-only
`.nav-cig-pack` skin forms a dark opened pack with torn foil and a mint-blue
edge. Inactive items are restrained internal pack sections. Only the active
button transforms into a cream paper cigarette: speckled filter behind the
icon, blue band, paper grain, ash, ember and smoke. It moves with `transform`,
not width or margin, so selection never reflows the rail.

`nav[data-burning]` (active DPI or proxy) increases ember heat and plume density.
Focus-visible remains explicit and all CSS animation is covered by the global
reduced-motion/frozen-scene gates. Rules remain scoped to
`[data-theme="yanineko"]`; the other themes keep the stock navigation.

## Unlock and persistence

The secret phrases remain normalized in `secretStore.ts`: `yanineko`,
`янинеко`, `янико`, `yaniko`, `яникас`. Theme restoration continues through
`themeStore.ts` and the existing `obsession.theme` localStorage key.

## Development harness and verification

`yani-dev.html` is a second Vite HTML entry. It renders the actual field, core
and nav without Tauri and exposes:

- all five states, three quality tiers and 240/220/104 px cores;
- WebGL2/Canvas forcing, pause and reduced-motion stills;
- 1000×680 and 800×600 composition presets;
- theme crossfade and `WEBGL_lose_context` simulation.

Unit coverage lives beside `yanineko/` and verifies state priority,
deterministic events, safe smoothing, core bounds, monotonic quality, panel
normalization, shader contracts and GPU cleanup/context loss. The four local
reference images remain design input only and never enter the bundle.
