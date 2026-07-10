# DreamCore Secret Theme Design

Date: 2026-07-10
Project: ObsessionTauri
Status: Approved design draft for user review

## Goal

Add a new secret visual theme named `DreamCore` to ObsessionTauri.

The theme should feel nostalgic and calm at first glance, but create sincere low-level anxiety through liminal emptiness, impossible quiet, dark openings, and slightly unnatural light. It must integrate with the existing frontend-only theme system and should not require backend migrations.

## Current Context

ObsessionTauri uses:

- Backend: Rust + Tauri v2.
- Frontend: React 18, TypeScript, Vite, Tailwind, Framer Motion, Zustand.
- Theme storage: `src/store/themeStore.ts`, persisted in `localStorage`.
- Secret unlocks: `src/store/secretStore.ts`, also persisted in `localStorage`.
- Fullscreen theme background dispatcher: `src/design/components/HeroField.tsx`.
- Central power-button dispatcher: `src/design/components/HeroCore.tsx`.
- Theme previews: `src/screens/Settings.tsx` and `src/design/components/Onboarding.tsx`.
- Theme CSS variables: `src/styles/globals.css`.

Backend settings do not currently store theme choice. Existing themes are frontend-owned and can respond to runtime state through `useDpiStore` and `useProxyStore`.

## References

User-provided DreamCore references are located at:

`C:\Users\biinn\Downloads\dreamcore`

The observed visual direction from the references:

- Dark liminal and backrooms-like spaces.
- Cold blue and cyan light.
- Muted gray, beige, brown, cream, and navy areas.
- Empty rooms, corridors, hard rectangular openings, and flat planes.
- One brighter sky-blue dreamlike reference.

The implementation should copy selected images into the app public assets folder rather than loading them from the user Downloads path at runtime.

## Recommended Approach

Use a 2D canvas and real bitmap references, not Three.js.

Reasoning:

- DreamCore works best as a still, familiar place with a living atmosphere.
- The app already has performant 2D canvas patterns for theme fields and cores.
- A 2D implementation is lighter than a new WebGL scene and easier to pause with `renderActive()`.
- Real photo references preserve the liminal feeling better than fully generated geometry.

## Theme Unlock

Add a secret reward:

- Reward id: `dreamcore`
- Title: `DreamCore`
- Keys: English only

Initial unlock keys:

- `dreamcore`
- `dream`
- `liminal`

No Cyrillic or translated unlock keys should be added.

## Theme Registry

Add a new theme id:

`dreamcore`

Update:

- `Theme` union in `src/store/themeStore.ts`
- `THEME_IDS`
- `THEMES`

Theme label:

`DreamCore`

Theme visibility:

Only visible after secret id `dreamcore` is unlocked.

## Assets

Create:

`ObsessionTauri/public/dreamcore/`

Copy selected reference images from:

`C:\Users\biinn\Downloads\dreamcore`

Use stable names such as:

- `room.jpg`

First implementation asset choice:

- Copy `Dreamcore.jpg` to `ObsessionTauri/public/dreamcore/room.jpg`.
- Use `/dreamcore/room.jpg` as the only runtime background image.
- Do not add image rotation in the first implementation.

Reasoning:

- `Dreamcore.jpg` is landscape-oriented, which fits the app window better than the vertical references.
- One stable image keeps the first implementation predictable and easier to tune behind UI panels.
- The canvas atmosphere layer will provide motion and state changes.

## Visual Direction

Balance:

- 70 percent nostalgic calm.
- 30 percent liminal anxiety.

Do:

- Use soft cold blue, cyan, milk white, faint peach, gray beige, and deep navy.
- Keep the UI readable and premium.
- Add film grain, dust motes, mild exposure breathing, and rectangular light patches.
- Use dark doorways or openings as quiet anxiety cues.
- Make animation slow and nearly subconscious.

Avoid:

- Jumpscares.
- Horror faces or explicit monsters.
- Loud flicker.
- Overly colorful pastel fantasy.
- Heavy purple-blue gradient dominance.
- Large decorative orbs unrelated to the scene.

## DreamCoreField

Create:

`src/design/components/DreamCoreField.tsx`

Responsibilities:

- Render a fullscreen liminal background image from `/dreamcore/...`.
- Apply cover positioning, subtle blur or softness, vignette, and color grading.
- Render a 2D canvas atmosphere layer:
  - Dust motes drifting slowly.
  - Very low opacity film grain.
  - Soft rectangular light bands.
  - Slight exposure breathing.
  - Optional dark doorway emphasis through gradients.
- React to runtime hot state:
  - `hot = dpi.active || proxy.running`
  - In hot state, light becomes steadier, slightly colder, and a little more lucid.
  - It should not become aggressive or celebratory.

Performance and behavior:

- Use `renderActive()` and `onRenderActiveChange()`.
- Cap canvas animation to about 30 fps.
- Use reduced internal canvas scale where appropriate.
- Respect `reduce_motion` through the existing render gate.
- Clean up resize and animation listeners.

## DreamCoreCore

Create:

`src/design/components/DreamCoreCore.tsx`

Responsibilities:

- Keep the same public props as existing core components:
  - `active`
  - `busy`
  - `onClick`
  - `size`
- Render a circular power button compatible with existing screens and previews.
- Inside the circle, render a simple luminous door or window opening.
- OFF state:
  - Dim, sleepy, soft blue-gray.
  - Door/window barely lit.
  - Very slow breathing.
- ON state:
  - Light is steadier and brighter.
  - Dust and edge glow become more visible.
  - The scene feels awake but still uneasy.
- Busy state:
  - Gentle unstable lamp flicker.
  - No harsh strobe.

Implementation details:

- Use canvas for the inner portal effect and dust.
- Use existing `motion.button` interaction pattern.
- Use `onRenderActiveChange()` and `renderActive()`.
- Keep the ON/OFF label style consistent with other cores.

## CSS Theme Variables

Add `[data-theme="dreamcore"]` to `src/styles/globals.css`.

Suggested variables:

- `--accent`: muted sky blue.
- `--accent-cyan`: pale cyan or milk blue.
- `--accent-violet`: faded lavender or soft peach-gray.
- `--glass-bg`: more milky, slightly fogged glass.
- `--glass-border`: pale blue-white with low opacity.
- `--glass-blur`: slightly higher than default.
- `--glass-sat`: restrained saturation.
- `--glass-specular`: soft milk-white highlight.

Also tune:

- `.text-gradient` under `[data-theme="dreamcore"]`
- scrollbar thumb colors

## Integration Points

Update:

- `src/store/secretStore.ts`
- `src/store/themeStore.ts`
- `src/design/components/HeroField.tsx`
- `src/design/components/HeroCore.tsx`
- `src/screens/Settings.tsx`
- `src/design/components/Onboarding.tsx`
- `src/styles/globals.css`

No Rust backend changes are required.

## Error Handling and Fallbacks

If an image path fails to load:

- The field should still show a CSS gradient background.
- Canvas atmosphere should continue to render.
- The app should not throw or blank the background.

If canvas context is unavailable:

- Return static layered CSS and image background.
- The theme remains selectable.

If the secret is already unlocked:

- Existing behavior should show the "already unlocked" message.

## Testing

Run:

```bash
npm run build
```

Manual checks:

- Enter `dreamcore` in the secret box.
- Confirm the theme unlocks and switches immediately.
- Confirm `DreamCore` appears in Settings only after unlock.
- Confirm the theme preview tile renders.
- Confirm Onboarding theme selection handles the new id if unlocked.
- Confirm full background and central core render.
- Confirm ON, OFF, and busy states look distinct.
- Confirm reduce motion stops canvas animation through existing render gate.
- Confirm no layout shifts in Settings preview tiles.

## Out of Scope

- Backend theme persistence.
- New Rust commands.
- Installer changes.
- Audio.
- Random scary events.
- Complex 3D scene.
- Runtime loading from `Downloads`.



