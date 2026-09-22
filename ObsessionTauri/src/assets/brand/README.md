# Peek cat identity

The user approved the peek-a-boo cat after inspecting actual 16–64px previews. This supersedes the earlier moth identity; moth files are retained as earlier artwork, not active assets.

- `peek-cat.png`: approved imagegen adaptation of the user-supplied `peek a boo.jpg`; identical to `artifacts/peek-cat-study/peek-cat.png`.
- Preserve its tilted face, uneven eyes, irregular outline, framing and warm light background. Do not regenerate it for routine icon exports.
- Generate sizes with `npx tauri icon src/assets/brand/peek-cat.png --output src-tauri/target/peek-cat-icons`.
- Copy root PNGs and ICO to `src-tauri/icons`, the ICO to `installer/src-tauri/icons/icon.ico` and `src-tauri/resources/icons/tray.ico`.
- Navigation uses the generated 128px PNG (64 KiB decoded RGBA). Installer artwork uses the generated 512px PNG. Neither uses a video decoder.
- Existing internal helper/component names are retained. Active and inactive tray states share the same icon; the tooltip and menu carry status.
