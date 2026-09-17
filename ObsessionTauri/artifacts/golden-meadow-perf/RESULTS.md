# Golden Meadow verification — 2026-09-17

## Changes

- Procedural scene builds and animates in a module worker using OffscreenCanvas.
- Distant grass and lighting are flattened into one retained background; temporary layers are released. Cat, leaves and foreground remain animated.
- Raster scale is capped at 1.5; at most one frame request is in flight.
- A 231,704-byte WebP of the scene is preloaded and displayed while the worker initializes. This removes the blank background; it does not eliminate worker initialization time.
- Overview protection card uses the standard compact eye icon instead of the pixel cat.

## Browser comparison

Same 871×958 viewport, DPR 1, 8-second DOM scrolling/transform workload per renderer. Main-thread setup: 962.7 ms before, 0.6 ms with worker (worker build 747.3 ms). p95 UI frame gap: 77.8 ms before, 11.1 ms after. Long tasks: one 963 ms task before, none after. Retained scene surfaces: 70.89 MiB before, 22.30 MiB after. Surface estimates are not total WebView2 memory. Rare large frame gaps occurred in both runs; this is a local diagnostic, not a guarantee of every frame's latency.

## Native Obsession / WebView2

Verified Golden Meadow selection, Settings scrolling and Overview card visually. Worker renderer active, scale 1.5, retained surfaces 23,380,556 bytes.

- Settings scroll at 09:46:27 UTC: p95 UI frame gap 5.6 ms, maximum 16.7 ms, no long tasks.
- Overview navigation at 09:46:59 UTC: p95 5.7 ms, maximum 44.3 ms, no long tasks.
- Theme switch itself still recorded several long tasks (68–102 ms); the poster addresses visible loading, not every cost of switching from another animated theme.

Temporary probe import and Vite capture endpoint were removed after measurement. Raw local measurements remain in native.jsonl.

## Checks

Production frontend build passed. Renderer, frame scheduler, screen transition and theme lifecycle suites: 21 tests passed. Native screenshot confirmed the protection card has the regular eye and compact height.
