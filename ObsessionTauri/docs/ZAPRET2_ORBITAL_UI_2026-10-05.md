# Zapret2: orbital interface · 2026-10-05

The service picker now occupies the DPI workspace instead of sharing a narrow
column with an always-visible log. Legacy retains its previous screen.

The primary view contains connection power, service selection, three search
modes, service cards and the next action. Profile selectors, provenance, rounds,
function tables and transport evidence are under **Настройка и результаты**.
Diagnostics and logs are separately expandable and mount when opened.

The scene uses sixteen projected torus-knot strands, three travelling light
strokes, an orbiting satellite and a breathing core. Geometry is generated once;
CSS animates SVG strokes, transforms and opacity. Search, confirmation and
restoration phases use the actual picker/scenario state. No simulated success or
time estimate is generated. The orbit pauses when offscreen, when the render
controller pauses, and with reduced motion. No external assets or new dependencies.

Confirmed network results offer explicit application and a separate guided
Discord/video check. Partial results offer another search. Existing application,
cancellation, scenario and confirmation guards remain in the store and backend.
The compact interface does not label network checks as working voice or video.

## Verification

- 42 existing frontend tests across picker UI, scenarios, store and Tauri bridge.
- TypeScript, Vite, native release and Setup builds.
- Browser QA at 1000×680, 800×600 and 1200×900, with two and three services.
- Picker start/cancel, confirmed/partial results, collapsed manual controls,
  expanded manual controls and reduced motion observed in the development fixture.
- Complete payload/source hash comparison, exact embedded payload and signed
  update manifest verification; see the build receipt.

The visual fixture is at `devtools/labs/zapret2-workspace.html`. Its controls use
local fake state and do not touch installed settings or the DPI service. It is
not part of the production bundle. Screenshots are in
`artifacts/zapret2-orbital-ui/` and explicitly show the fixture label.

The installed application was not replaced. No live network test was started for
this appearance change. Native WebView rendering and low-end GPU performance have
not been measured. Runtime files, candidate recipes and the TLS v4 probe policy
are byte-identical to the previous working TLS release; the build verifier checks
this against `ZAPRET2_TLS_SHAPES_BUILD_2026-10-05.json`.

Installer: `dist-release/experimental/zapret2-orbital-ui-2026-10-05/Obsession-Setup_1.1.0_x64.exe`.
SHA-256 and source receipts: `ZAPRET2_ORBITAL_UI_BUILD_2026-10-05.json`.
