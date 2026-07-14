# Zapret2 Gate D Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Исправить пустой первый профиль Zapret2, одновременно обеспечить Discord text + стабильный YouTube TLS/QUIC, затем закрыть resource-integrity и crash-fallback части Gate D.

**Architecture:** Аргументы `winws2` строятся из типизированного invocation: глобальные Lua/blob/capture параметры идут один раз, полностью описанные профили разделяются `--new` только между ними. Встроенный Strategy Pack становится самоописательным для TLS/QUIC, а runtime проверяет бинарные ресурсы до запуска и использует generation-safe fallback на последний подтверждённый Legacy-набор.

**Tech Stack:** Rust 2021, Tokio, Tauri 2, serde/serde_json, sha2, TypeScript, Zustand, Vite, Zapret2 1.0.2/WinDivert.

---

## File map

- Modify `ObsessionTauri/src-tauri/src/dpi_engine/zapret2.rs`: типизированный invocation и правильная сериализация профилей.
- Modify `ObsessionTauri/src-tauri/src/dpi_engine/manifest.rs`: поля L7/payload/ranges/blobs и их валидация.
- Modify `ObsessionTauri/src-tauri/src/dpi_engine/mod.rs`: выбор всех профилей одного уровня и загрузка pack blobs.
- Create `ObsessionTauri/src-tauri/src/dpi_engine/resources.rs`: проверка общего binary resource manifest.
- Modify `ObsessionTauri/src-tauri/src/dpi.rs`: сбор invocation, stop-before-start, diagnostics, generation-safe crash monitor.
- Modify `ObsessionTauri/src-tauri/src/commands.rs`: availability по manifest и корректный early fallback.
- Modify `ObsessionTauri/src-tauri/src/state.rs`: DPI generation и last confirmed Legacy selection.
- Modify `ObsessionTauri/src-tauri/src/paths.rs`: изолированный путь `bin/zapret2/winws2.exe` и extraction anchor.
- Modify `ObsessionTauri/src-tauri/resources/strategy-packs/builtin/manifest.json`: Discord TLS, YouTube TLS и YouTube QUIC профили.
- Create `ObsessionTauri/src-tauri/resources/strategy-packs/builtin/blobs/*.bin`: проверенные TLS/QUIC payloads.
- Modify `ObsessionTauri/src-tauri/resources/manifest.json`: фактическая изолированная раскладка Zapret2.
- Move/copy Zapret2 runtime to `ObsessionTauri/src-tauri/resources/bin/zapret2/`.
- Modify `ObsessionTauri/src/store/dpiStore.ts`: обновлять engine selection после fallback status.

### Task 1: Correct the winws2 profile grammar

**Files:**
- Modify: `ObsessionTauri/src-tauri/src/dpi_engine/zapret2.rs`

- [ ] **Step 1: Write regression tests for delimiter placement**

Add tests asserting that global arguments precede profiles, the first profile starts with `--name=...`, two profiles contain exactly one bare `--new`, and no trailing delimiter exists:

```rust
#[test]
fn two_profiles_use_one_separator_without_empty_first_profile() {
    let args = build_winws2_args(&invocation(vec![profile("discord"), profile("youtube")]));
    assert_eq!(args.iter().filter(|a| a.as_str() == "--new").count(), 1);
    let first_name = args.iter().position(|a| a == "--name=discord").unwrap();
    let separator = args.iter().position(|a| a == "--new").unwrap();
    let second_name = args.iter().position(|a| a == "--name=youtube").unwrap();
    assert!(first_name < separator && separator < second_name);
    assert_ne!(args.last().map(String::as_str), Some("--new"));
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test dpi_engine::zapret2::tests::two_profiles_use_one_separator_without_empty_first_profile -- --exact`

Expected: FAIL because the current builder emits `--new=<name>` before both profiles.

- [ ] **Step 3: Introduce structured invocation types**

Implement:

```rust
pub struct BlobArg { pub name: String, pub path: String }
pub struct Zapret2Invocation {
    pub wf_tcp_out: Option<String>,
    pub wf_udp_out: Option<String>,
    pub lua_init: Vec<String>,
    pub blobs: Vec<BlobArg>,
    pub profiles: Vec<Zapret2Profile>,
}

pub struct Zapret2Profile {
    pub name: String,
    pub filter_tcp: Option<String>,
    pub filter_udp: Option<String>,
    pub filter_l7: Vec<String>,
    pub hostlist: Option<String>,
    pub payload: Vec<String>,
    pub out_range: Option<String>,
    pub in_range: Option<String>,
    pub lua_desync: Vec<String>,
}
```

Serialize global arguments once, emit each complete profile, and append bare `--new` only when another profile follows.

- [ ] **Step 4: Run all Zapret2 builder tests**

Run: `cargo test dpi_engine::zapret2::tests`

Expected: all builder tests PASS.

### Task 2: Extend and validate the built-in Strategy Pack

**Files:**
- Modify: `ObsessionTauri/src-tauri/src/dpi_engine/manifest.rs`
- Modify: `ObsessionTauri/src-tauri/src/dpi_engine/mod.rs`
- Modify: `ObsessionTauri/src-tauri/resources/strategy-packs/builtin/manifest.json`
- Create: `ObsessionTauri/src-tauri/resources/strategy-packs/builtin/blobs/tls_clienthello_www_google_com.bin`
- Create: `ObsessionTauri/src-tauri/resources/strategy-packs/builtin/blobs/quic_initial_www_google_com.bin`

- [ ] **Step 1: Write failing schema validation tests**

Cover valid blob declarations, undeclared blob paths, invalid blob names, unknown `filter_l7`, unknown `payload`, empty desync, and multiple profiles sharing the selected aggressiveness.

```rust
#[test]
fn rejects_unknown_l7_and_payload() {
    let (mut manifest, files) = valid_manifest();
    manifest.strategies[0].filter_l7 = vec!["not-a-protocol".into()];
    manifest.strategies[0].payload = vec!["not-a-payload".into()];
    let report = validate_pack(&manifest, caps(), resolver(&files));
    assert!(report.errors.iter().any(|e| e.contains("filter_l7")));
    assert!(report.errors.iter().any(|e| e.contains("payload")));
}
```

- [ ] **Step 2: Run focused manifest tests and verify failure**

Run: `cargo test dpi_engine::manifest::tests`

Expected: compile/test failure because the new schema fields do not exist.

- [ ] **Step 3: Add typed manifest fields**

Add `BlobDef { name, path }`, `blobs: Vec<BlobDef>` on the pack, and these fields on `StrategyDef`:

```rust
#[serde(default)] pub filter_l7: Vec<String>,
#[serde(default)] pub payload: Vec<String>,
#[serde(default)] pub out_range: Option<String>,
#[serde(default)] pub in_range: Option<String>,
```

Allow only known values required by the pack (`tls`, `quic`, `tls_client_hello`, `quic_initial`), validate blob names as ASCII identifiers, require blob paths in `files`, and reject profiles without desync actions.

- [ ] **Step 4: Make level selection return every matching profile**

Add:

```rust
pub fn profiles_for(&self, category: &str, requested_level: u8) -> Vec<StrategyDef>
```

Choose the requested aggressiveness when present; otherwise choose the maximum available level, then return every profile at that level in manifest order. This permits YouTube TLS and QUIC to run together.

- [ ] **Step 5: Copy and hash the built-in blobs**

Copy the existing trusted resources into the pack-local `blobs/` directory without altering bytes. Record their existing SHA-256 values in `files` and declare `tls_google` / `quic_google` in `blobs`.

- [ ] **Step 6: Replace draft strategies with the approved profiles**

Use aggressiveness level 1 for all three profiles:

```json
{
  "id": "discord_tls_text",
  "category": "discord",
  "filter_l7": ["tls"],
  "payload": ["tls_client_hello"],
  "out_range": "-d10",
  "desync": [
    "fake:blob=tls_google:tcp_ts=-30000:tcp_ts_up:repeats=4",
    "multisplit:pos=1"
  ]
}
```

```json
{
  "id": "youtube_tls",
  "category": "youtube_twitch",
  "filter_l7": ["tls"],
  "payload": ["tls_client_hello"],
  "out_range": "-d10",
  "desync": [
    "fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com",
    "multidisorder:pos=1,midsld"
  ]
}
```

```json
{
  "id": "youtube_quic",
  "category": "youtube_twitch",
  "transports": ["udp", "quic"],
  "filter_l7": ["quic"],
  "payload": ["quic_initial"],
  "desync": ["fake:blob=quic_google:repeats=11"]
}
```

- [ ] **Step 7: Run pack tests**

Run: `cargo test dpi_engine::manifest::tests dpi_engine::tests::loads_real_builtin_pack_with_valid_integrity`

Expected: all selected tests PASS and real pack hashes match.

### Task 3: Wire the invocation into runtime and diagnostics

**Files:**
- Modify: `ObsessionTauri/src-tauri/src/dpi.rs`

- [ ] **Step 1: Build all selected profiles, Lua libraries and blobs**

Replace the single `.find()` strategy selection with `pack.profiles_for(category, level)`. Collect unique global Lua files, absolutize pack-local blob paths, build one `Zapret2Invocation`, and set capture to TCP/443 when any TCP profile exists and UDP/443 when any UDP profile exists.

- [ ] **Step 2: Stop any owned DPI runtime before Zapret2 spawn**

Call `stop_all(app).await` before constructing/spawning the new process, then re-check shutdown. This makes the backend safe even when called outside the current UI guard.

- [ ] **Step 3: Add deterministic startup diagnostics**

Log pack id/version, ordered profile ids, capture ports, and the final argv joined for diagnostics. Do not log secrets; this invocation contains only built-in paths and strategy values.

- [ ] **Step 4: Run focused tests and compile check**

Run: `cargo test dpi_engine::zapret2::tests dpi_engine::manifest::tests`

Run: `cargo check`

Expected: PASS with no new compiler errors.

### Task 4: Isolate Zapret2 resources and enforce the binary manifest

**Files:**
- Create: `ObsessionTauri/src-tauri/src/dpi_engine/resources.rs`
- Modify: `ObsessionTauri/src-tauri/src/dpi_engine/mod.rs`
- Modify: `ObsessionTauri/src-tauri/src/paths.rs`
- Modify: `ObsessionTauri/src-tauri/src/commands.rs`
- Modify: `ObsessionTauri/src-tauri/src/dpi.rs`
- Modify: `ObsessionTauri/src-tauri/resources/manifest.json`
- Move/Create: `ObsessionTauri/src-tauri/resources/bin/zapret2/winws2.exe`
- Move/Create: `ObsessionTauri/src-tauri/resources/bin/zapret2/WinDivert.dll`
- Copy/Create: `ObsessionTauri/src-tauri/resources/bin/zapret2/WinDivert64.sys`

- [ ] **Step 1: Write resource validator tests**

Test valid engine resources, missing file, size mismatch, SHA-256 mismatch, unsafe `../` path and unknown engine id using a unique temp directory.

- [ ] **Step 2: Implement `validate_engine_resources`**

Parse the existing top-level `resources/manifest.json`, resolve only safe relative paths under the AppData base directory, and validate size/SHA-256 for the selected engine. Return a structured error string used by UI availability and start paths.

- [ ] **Step 3: Move Zapret2 into its own userspace runtime directory**

Keep Legacy at `bin/winws.exe` with `bin/WinDivert.dll`. Place Zapret2 at `bin/zapret2/winws2.exe` beside the official Zapret2 `WinDivert.dll` and the shared byte-identical driver. Update `Paths::winws2_path()` and `missing_key_asset()`.

- [ ] **Step 4: Correct the resource manifest**

Mark Zapret2 active and change paths to `bin/zapret2/...`. Remove the stale note claiming files are not copied. Keep exact published hashes.

- [ ] **Step 5: Enforce validation before availability/start**

`dpi_engine_list` sets `available` from validation, `dpi_engine_set` rejects invalid resources, and both Legacy/Zapret2 start paths validate their engine before spawn.

- [ ] **Step 6: Run resource and path tests**

Run: `cargo test dpi_engine::resources::tests paths::tests`

Expected: PASS.

### Task 5: Add generation-safe Zapret2 crash fallback

**Files:**
- Modify: `ObsessionTauri/src-tauri/src/state.rs`
- Modify: `ObsessionTauri/src-tauri/src/dpi.rs`
- Modify: `ObsessionTauri/src-tauri/src/commands.rs`
- Modify: `ObsessionTauri/src/store/dpiStore.ts`

- [ ] **Step 1: Write pure state tests**

Test generation wrap avoidance, stale generation rejection, intentional stop rejection, and retrieval of the last confirmed Legacy selection.

- [ ] **Step 2: Extend DPI runtime state**

Add monotonic `generation: u64`, `last_legacy_selection: Vec<(String, String)>`, and `generation`/engine metadata on owned processes. Every start/stop invalidates older callbacks.

- [ ] **Step 3: Record Legacy selection only after successful start**

After `start_many` starts at least one Legacy process, store the exact started category/config pairs as the fallback candidate. Zapret2 starts never overwrite it.

- [ ] **Step 4: Connect the Zapret2 monitor to fallback**

On unexpected exit, remove only the matching PID/generation, acquire `dpi_gate`, re-check generation/shutdown/intentional stop, persist engine selection `legacy`, and call `start_many` with the saved selection. If none exists, leave DPI stopped and log/notify an explicit error.

- [ ] **Step 5: Correct early-start fallback selection**

When Zapret2 fails during the first 500 ms and commands fall through to Legacy, persist `dpi_engine=legacy` before starting Legacy.

- [ ] **Step 6: Refresh frontend engine chips after status events**

In the existing DPI status listener, call `loadEngines()` after applying process status so a crash fallback visibly selects Legacy.

- [ ] **Step 7: Run lifecycle tests**

Run: `cargo test dpi_engine::tests state::tests`

Expected: PASS.

### Task 6: Full gates, dev build and launch

**Files:** all files above.

- [ ] **Step 1: Format and run Rust tests**

Run: `cargo fmt -- --check`

Run: `cargo test`

Expected: all tests PASS; test count does not decrease from the current source baseline.

- [ ] **Step 2: Build frontend**

Run: `npm run build`

Expected: TypeScript and Vite build PASS.

- [ ] **Step 3: Check repository hygiene**

Run: `git diff --check`

Expected: no whitespace errors. `dist/` and `target/` remain ignored.

- [ ] **Step 4: Build the Tauri dev executable**

Run: `cargo build` from `ObsessionTauri/src-tauri`.

Expected: `target/debug/obsession.exe` is rebuilt successfully with bundled resources staged.

- [ ] **Step 5: Launch dev mode**

Run: `npm run tauri dev` from `ObsessionTauri` in a background helper process while leaving the application window visible.

Expected: the dev application opens, extracts `bin/zapret2/*` and the updated built-in pack into `%APPDATA%/Obsession`, and remains running for the user's Discord/YouTube acceptance test.

- [ ] **Step 6: Inspect startup state without altering user traffic**

Verify the Obsession process is running, no orphan `winws2.exe` exists before the user presses Start, and recent logs contain no resource-manifest or pack-integrity error.
