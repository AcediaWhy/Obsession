# Moth identity integration

The user selected the supplied ink moth drawing as the new Obsession symbol. The source is preserved at `ObsessionTauri/src/assets/brand/moth-reference.jpg`; the two derived PNG assets and reproduction instructions are alongside it. The imagegen adaptations preserve the downward wings and eye spots; they are not claimed to be pixel-identical to the supplied drawing.

## Changes

- Static moth in the application navigation; no video, media subscriptions or decoder for the brand mark. Uses a 128px PNG rather than decoding the full-resolution artwork at 40px.
- Windows application and custom-installer icons replaced; the tray ICO uses the same new mark in both runtime states. Its 16, 24, 32, 48, 64 and 256px entries are present.
- Ink moth replaces the animated eye on the installer welcome, preparation, progress and completion screens.
- Removed the completion success circle and duplicate green status badge. Reduced surrounding decoration: no ambient color orbs, logo glow, sheen or animated branding; primary buttons now use an ivory fill.
- Fixed the development-only welcome preview to leave the initial bootstrap state.

Installation, update, repair, rollback and DPI logic are unchanged in this work. The existing four-step installation flow is retained; this change integrates the identity and cleans up presentation rather than replacing that flow.

## Validation

GitNexus upstream analysis: `EyeLogo` has one direct caller (NavRail), `eyeHtml` one (render), installer `render` four, and `applyPreviewState` one. All are LOW risk. The aggregate uncommitted worktree remains critical because it includes the earlier runtime/DPI changes.

Installer state tests: 5 passed. Browser preview inspected at the actual 720×500 window size for welcome, options, progress, completion and error. Completion has zero video elements and zero success-badge elements, its artwork loads, and the document has no horizontal overflow. Production TypeScript/Vite compilation is part of the custom release build.
