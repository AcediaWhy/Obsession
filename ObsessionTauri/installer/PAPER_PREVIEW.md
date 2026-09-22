# Paper / cat installer study

The installer retains its existing native installation, update, repair and error handling. Only the presentation is revised: the approved cat icon, a short greeting, shortcut cards, an expandable installation path, paper-like entrance motion and quieter progress UI. No actual installation is performed by the `?preview=welcome` development route.

Papyrus is loaded via `local("Papyrus")`. The user's installed font has Cyrillic glyphs, verified with Windows GlyphTypeface, and was visually checked in the preview. No font file was copied into the repository. Redistribution rights need confirmation before bundling the supplied font; without a local installation, the CSS falls back to Georgia.

The cat reuses `src-tauri/icons/icon.png`; the previous 1.1 MB moth asset is no longer imported by the installer. The paper grain is CSS/inline SVG, not an additional bitmap. Payload compression is outside this visual pass.

Validation: installer TypeScript/Vite build and five installer-state tests passed. At 720×500, inspected welcome/options/completion/error screens and exercised the simulated install flow. Shortcut choices persist when going back and forward; the destination disclosure exposes the fixed Program Files path. Existing system reduced-motion rules override the new CSS animations and transitions. Native EXE was not rebuilt.

GitNexus impact for `render` and `modeCopy`: LOW. Direct render callers are startInstall, launchApp, loadSnapshot and the module; modeCopy feeds render. No indexed execution processes were reported. Installation functions were not changed. Humanizer-ru guided removal of corporate filler and unsupported timing promises from user-facing copy.
