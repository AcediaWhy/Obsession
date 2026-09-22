# Fallen Down: ruins garden, second pass

Original background generated with the built-in image_gen tool on 2026-09-19. Active project asset: src/assets/fallendown/ruins-garden-v3.png (1536 × 1024), edited on 2026-09-20 to repair the flowerbed. The v2 source is retained for comparison. No game assets are bundled.

## References and findings

- [Official Undertale landing-room screenshot](https://undertale.com/assets/images/screen2.png): strong separation of floor and void, bright flowerbed as the focal point.
- [Ruins rooms and geography](https://undertale.wiki/w/Ruins): purple masonry, vines, red leaves, stairs, dark openings, golden flowers at the beginning.
- [Community room reconstruction](https://github.com/Bli-AIk/open-utdr-maps/blob/main/curated/undertale/room_castle_prebarrier.tmx): search reference only, not an official source release and no code/assets imported.

The first implementation used large flat SVG arches and a regular array of flowers. The replacement is an original interpretation with connected architecture, tile materials, grouped vegetation and directional light. It is more detailed than Undertale's native low-resolution graphics, not a claim of pixel-for-pixel fidelity.

## Runtime

A single static local image replaces the vector scene. HeroField unmounts the whole scene when hidden; no image preloading, new animation timer, video, WebGL scene, or new dependency. Existing particles and visibility/reduced-motion behavior are retained. The decoded RGBA image budget is about 6 MiB when resident; native process/tray memory must be measured separately rather than inferred from file size. The independent save-point SVG follows protection status. Production packaging uses Vite's asset pipeline.

## Ambient motion — 2026-09-20

The same source-coordinate plane now holds the background and a small SVG atmosphere: 22 deterministic golden dust motes, a nine-second change in beam brightness, and a small pool of warm light. The save star shimmers over 3.6 seconds; active protection adds a single 950 ms flash that fades completely. No additional bitmap, canvas, JavaScript timer or animation-frame loop was added.

FallenField gates the CSS animations with useRenderActive and its paused prop. Reduced motion leaves a still composition; HeroField still unmounts the scene on native hide. Defensive hidden-document and system-reduced-motion CSS rules also stop the new effects. Native tray process memory was not measured in this iteration.

Validation: production build passed; browser inspection confirmed changing transforms/opacity, all four new effects disabled under reduced motion, one flash iteration ending at zero opacity, and matching image/overlay bounds at 1000 × 680.

## Rejected flower cutouts — 2026-09-20 (removed)

User inspection exposed hard seams and partially clipped adjacent flowers. The cutouts and underlay described below have been removed, including the unused underlay file. Computed animation checks were insufficient visual validation. This section records the failed approach, not current runtime behavior.

Four small irregular cutouts reuse the existing artwork and rotate around their bases by at most 1.7 degrees, with independent 6.8–9.1 second periods and negative delays. Each cutout has a small SVG viewport; no full-room transform, filter, extra canvas, JavaScript timer, or animation-frame loop is introduced. The paving and the remaining flowers stay still.

A built-in image_gen edit produced `src/assets/fallendown/flower-underlay.png` (1536 × 1024, 1,981,066 bytes). It reconstructs foliage behind the blooms. Only four clipped patches are rendered beneath the moving originals; the rest of the generated edit is never shown. The original room asset remains unchanged. The additional decoded bitmap budget is approximately 6 MiB, excluding compositor surfaces and browser caching; this is not a measured native-process delta.

Flower animations use the existing `data-fallen-motion` gate and stop with app/system reduced motion and hidden-document rules. HeroField continues to unmount the scene on hide. Browser checks confirmed four changing transforms and all four returning to `animation: none; transform: none` under reduced motion. The production build passed. Native tray memory was not measured.

### Underlay edit prompt (built-in image_gen)

Use case: precise-object-edit. Asset type: animation clean plate for an existing pixel-art background. Edit the supplied 1536x1024 ruins image. Remove ONLY the golden yellow flower heads and their stalks in the main lower-right flowerbed (approximately x=760..1510, y=560..795), reconstructing the dark green leafy ground behind them. Keep the bed as low dense dark green foliage, no golden blossoms there. Preserve every other part of this composition exactly: walls, stairs, vines, sunlight, purple stone paving, camera framing, pixel grid, and scattered petals lying on the stone floor. No new flowers, objects, light effects, text or UI. This is an invisible underlay revealed by tiny flower movements, so the shapes and dark green palette must match the original flowerbed ground very closely. Same full image dimensions and alignment, no crop, no reframing.

## Rejected flower sprites — 2026-09-20 (removed)

The user rejected the added plants as visually out of place. All four sprites, their CSS, their import, and the atlas file have now been removed. The description and prompt below are historical only.

The room now stays completely intact. Four additional true-alpha plant sprites sit inside the front edge of the foliage; no room pixels, paving, or adjacent flowers are transformed. The original painted bed itself stays still. `src/assets/fallendown/flower-sprigs.png` is a 1254 × 1254 RGBA atlas generated with built-in image_gen. Transparent corner and center pixels were verified. Four CSS background positions select its quadrants without editing or resampling the generated bitmap.

Independent 4.7–6.1 second rotations reach 3.5–4.5 degrees about the roots. No new JS loop, canvas, filter, or dependency. The existing motion/hidden gates remain. Decoded atlas budget is about 6 MiB, replacing the previous underlay budget; actual native memory has not been measured. Browser inspection confirmed zero old patch elements, four moving sprites, and all four stopping under reduced motion. Production build passed.

### Sprite generation prompt (built-in image_gen)

Use case: stylized-concept. Create a production transparent PNG sprite atlas for the attached pixel-art ruins scene. Reference image is STYLE AND PALETTE REFERENCE ONLY. Output 1024x1024, true transparent alpha background, four different small golden flower sprigs arranged in a precise 2x2 grid with one complete sprig centered in each 512x512 cell, generous 80px transparent margins in each cell. Each sprig has just 2-3 golden yellow five/six-petalled flowers, amber centers, thin dark olive stems and a few dark olive leaves at the base. Match the chunky pixel-art flowers in the lower-right bed of the reference: square stepped pixels, limited yellow/amber/cream and deep olive palette, no outlines. Sprigs roughly as tall as wide, viewed slightly from above, upper-left warm light. Fully isolated plant silhouettes INCLUDING transparent gaps between stems and leaves. No ground patch, no soil, no rectangular backing, no shadows outside silhouettes, no purple floor, no environment, no labels, no checkerboard drawn into the image, no soft glow. Four distinct organic asymmetrical arrangements, plants must not touch cell edges. These sprites will individually sway from their roots over the intact reference illustration.

## Current atmosphere — 2026-09-20

The original room and flowerbed remain completely intact and static. There are no additional flower cutouts, sprites, or underlay images in the scene or production bundle.

The atmosphere now uses a layered shaft of warm light with a 12-second opacity cycle, a radial light pool over the flowerbed, 28 drifting dust motes, and 12 small rising golden sparks with independent negative delays. Two groups of radial-gradient floor haze drift over 24 and 31 seconds. Cooler edge shadows add depth. SVG gradients replace hard ellipse edges without blur filters. Existing save-point shimmer and protection flash remain.

All animation selectors require the existing motion gate. Reduced motion retains still light/haze, stops dust/save shimmer and hides the rising sparks. Hidden-document CSS stops animations defensively, and HeroField still unmounts the scene on native hide. No additional canvas, JS frame loop, video, bitmap, or dependency. Removing the flower atlas removes roughly 6 MiB of decoded bitmap budget; native process memory is not inferred from that estimate.

## Completion verification — 2026-09-20

Resumed the interrupted atmosphere pass and verified the existing implementation without further visual changes. Inspected the running production components in the development overview at the default 871 × 958 viewport and at 1000 × 680. The flowerbed remains continuous, with no added flower sprites or cutout seams; the image and atmosphere have identical bounds at the compact size. Light opacity and haze/spark transforms change while motion is enabled. App reduced motion leaves zero CSS animations in the atmosphere/save-point layers and zero visible sparks. Re-enabling motion restores the effects. The preview protection toggle triggers one save-point flash, which ends at zero opacity. Browser error logs were empty.

Fresh `npm run build` (TypeScript + Vite) and the scoped `git diff --check` passed. The production assets contain the room image, and no removed flower atlas references remain in `src`. No new native installer was built during this completion pass; native tray memory was not measured.

GitNexus `impact FallenRuins --direction upstream` returned `UNKNOWN` because the new component is absent from the index. Source inspection confirms the route `FallenRuins → FallenField → HeroField`; HeroField retains its hidden-scene unmount. `detect_changes --scope all` ran and reported critical risk across the pre-existing working tree (75 files, 133 indexed symbols, 35 processes, including runtime/network work). That aggregate report cannot certify this unindexed visual component in isolation. No commit was created.

## Flowerbed repair — 2026-09-20

User close-ups revealed rectangular-looking breaks in petals and foliage that the preceding whole-scene inspection missed. The absence of runtime cutouts did not establish that the source artwork itself was free of slicing artifacts. The built-in image_gen tool repainted the connected flowerbed, preserving the room composition and lighting. FallenRuins now imports the versioned v3 asset; the original v2 remains available. Existing atmospheric overlays and animation gates are unchanged. The updated image was inspected in the live overview, including its petals and connected foliage.

Final edit prompt (built-in image_gen; v2 supplied as the edit target):

> Use case: precise-object-edit. Asset type: production background image for Fallen Down desktop app theme. Input image 1 is the EDIT TARGET. Repair ONLY the golden flowerbed in the lower right (roughly x=730..1535, y=560..810 of the 1536x1024 original). The existing flowers and foliage have jarring rectangular sliced/tiled patch edges, clipped petals, square bands and incoherent pixel clusters. Carefully repaint the entire connected flowerbed as one continuous organic clump: complete readable golden flower heads with intact irregular petals, natural occlusion between neighboring flowers, connected dark green leaves and small stems, consistent crisp pixel-art scale. Keep the same overall flowerbed silhouette, location, density, golden/amber palette and brightest area under the beam. Pixel steps should describe petals and leaves, not arbitrary rectangular patches or a mosaic. No repeating tiles, no horizontal or vertical slice boundaries, no blurry repair, no shiny plastic, no new foreground plants. Preserve ALL surrounding architecture, masonry, stairs, doorway, floor, loose petals, vines, camera framing, light shaft and shadows as faithfully as possible. Keep the exact full-room composition and aspect ratio, output 1536x1024. Do not crop, zoom or move the scene. No added particles, stars, mist, text, UI or characters. Deliver the repaired full background image, not a comparison sheet.

## Flowerbed rendering correction — 2026-09-21

The user clarified that the defect was straight horizontal bands crossing several flowers, not the stepped outlines of pixel-art petals. Repainting v2 as v3 did not resolve that report; the earlier source-art diagnosis was not established.

Compared both supplied close-ups with matching regions of v3 and an enlarged live overview. The reported bands fall at approximately the same scene height. The corresponding source regions do not show the same obvious straight break, suggesting a rendering contribution rather than proving a defect in the bitmap.

`fallenRuins.css` now uses normal image sampling (`image-rendering: auto`) for the continuously scaled illustration and positions the shared image/atmosphere plane with calculated `left`/`top` offsets instead of a fractional translate transform. The existing cover size and 64%/56% focal point (68%/top on short windows) are retained. No blur filter, new image generation, or atmosphere changes were added. Normal sampling makes the scaled artwork slightly softer.

Validation: inspected the enlarged live comparison and full scene at 871 × 958 and 1000 × 680; the reported straight band was not reproduced in those checks. Image and atmosphere bounds match at both sizes, and the shared plane has no transform. TypeScript/Vite production build and `git diff --check` passed. This is browser validation, not a native WebView/installer verification or a guarantee across all display scales. The local diagnostic page is `artifacts/fallen-image-check.html`.

GitNexus's whole-working-tree change report still includes the unrelated pre-existing runtime/network changes (75 files, 133 symbols, 35 processes; critical aggregate risk). This correction changes CSS, not indexed functions. No commit or installer was created.

## Additional atmosphere — 2026-09-21

Added nine deterministic fireflies hovering above the flowerbed, each with independent drift and glow timing. Two feathered radial-gradient light shafts vary slowly in brightness; a warm pool on the paving expands subtly, and a separate foreground haze drifts in front of the existing distant mist. The bitmap and its corrected sampling/positioning remain unchanged. No flower cutouts, new bitmap assets, filters, dependencies, JavaScript timers, or render loops were added.

New effects live inside the existing SVG and use the same motion gate, hidden-document rule and system reduced-motion rule. With app reduced motion enabled, browser inspection confirmed zero animated atmosphere/save-point elements and zero visible fireflies. Re-enabling motion restores them. Successive samples confirmed changing light opacity, haze transforms and firefly positions. The TypeScript/Vite production build passed. GitNexus impact for FallenRuins is still UNKNOWN (unindexed); the indexed parent FallenField reports LOW, with HeroField as its direct caller, three upstream symbols and two affected processes. Native performance/memory was not measured.

## Original generation prompt

Use case: stylized-concept. Asset type: production background illustration for a desktop application's Fallen Down theme. Create an original, exceptionally well composed 2D PIXEL ART underground ruins room strongly inspired by the visual vocabulary and melancholic warmth of Undertale's Ruins, without any characters, text, logos or UI. Landscape 3:2 aspect ratio. This is a real room from a pixel RPG, a slightly elevated orthographic view, NOT a frontal cathedral wallpaper. Coherent chunky pixel grid, crisp stair-step outlines, deliberately placed clusters of pixels, limited 18-24 color palette. Saturated dusty plum/violet brick masonry, lavender stone highlights, deep almost-black doorways, muted dark green climbing vines, sparse wine-red fallen leaves. Composition purpose: top 65 percent and left 22 percent sit behind app controls so keep those areas relatively low contrast but materially detailed (subtle brick courses, recessed niche, broken edges); put the most beautiful and readable storytelling in the lower right third, which remains uncovered. A low wide stone stair coming down from a dark arched opening on the RIGHT edge, with a broken parapet and creeping vines. Beneath it, a broad irregular bed of densely overlapping brilliant golden-yellow flowers on deep green leaves catches a single shaft of pale warm light from above. Flowers must form a lush organic pixel cluster, different sizes and directions, NOT identical repeated crosses on sticks or neat rows. Clearly defined purple tiled floor in lower third, short mossy borders, a few cracks and loose petals lead toward the flowerbed. Dark quiet floor space lower left, close foreground stone fragments bottom corners, readable middle-distance masonry. Some distant architectural detail visible at the far top/right edge. Beautiful constrained pixel shading in 3-4 flat steps, no soft gradients, no bloom, no smooth vector art, no isometric cube scene, no photorealism, no blur, no watercolor, no fake pixel filter, no noisy tiny details. The scene should feel lovingly hand-authored, inviting, nostalgic, a place one could walk into. Do not add hearts, save-point stars or floating particles; those are separate interactive overlays. Full bleed single background, no borders. High craft and intentional composition; darker upper area transitions naturally to illuminated golden flowers in bottom-right.
