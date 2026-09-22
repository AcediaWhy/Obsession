# Approved peek-cat identity

The user selected and approved the peek-a-boo cat in the 16–64px study. Its approved PNG is copied byte-for-byte into `src/assets/brand/peek-cat.png`. The light background and framing are intentional and unchanged.

Integration replaces Windows application, installer and tray icons, the navigation image and the installer artwork. The navigation uses the generated 128px image; the installer uses 512px. Both are static, with no logo video decoder. Existing tray active/inactive status remains in tooltips and menu items.

The earlier moth artwork remains on disk as an archived concept; the active imports and Windows icon files now use the cat. Historical internal component/helper names are retained.

Validation: installer TypeScript/Vite build and all five state tests passed; the 720×500 welcome/completion previews were inspected. All ICOs contain 16, 24, 32, 48, 64 and 256px entries. Upstream GitNexus analysis classified `EyeLogo` (NavRail caller) and `eyeHtml` (installer render caller) as LOW risk. The broader uncommitted worktree includes earlier DPI/runtime changes; this identity change does not alter those systems.
