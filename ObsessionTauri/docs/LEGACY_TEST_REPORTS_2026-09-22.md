# Legacy diagnostic reports

## Confirmed false-negative mechanisms

User log at 14:32 showed Discord14 rejected for HTTP 403 at the CDN root, and
YouTube11 rejected on a transport error loading a ytimg thumbnail. The old test
returned a single boolean and stopped at the first failed target.

## Changes

- Use `https://cdn.discordapp.com/embed/avatars/0.png`, a documented public default
  avatar, not the CDN root. Reference:
  https://github.com/discord/discord-api-docs/blob/main/developers/reference.mdx
- Download the whole PNG/JPEG within a 256 KiB bound, require expected HTTPS host
  and file markers, reject HTTP errors, HTML and truncated content. Preserve strict
  gateway/update-manifest validation. Generic pages are bounded 16 KiB samples.
- Collect every target result, with elapsed time, successful body bytes, attempts,
  HTTP status and transport error chain. Retry once except for HTTP 4xx.
- Return typed `passed`, `partial`, `failed`, `cancelled` reports from `dpi_test`.
  Update frontend/backend together; the privileged service protocol is unchanged.
- Show per-target details and avoid describing a failed probe as a broken config.
  Autopick records only complete success and preserves the previous choice when
  no candidate passes every test. Partial candidates remain visible, not blessed.
- Cancellation still stops exactly the test generation; stop failures abort the
  picker. Active user sessions are never stopped by this diagnostic entry point.

## Scope and evidence

This checks specific endpoints, not all media, voice or video playback. Timings
include connection establishment and retries, not just payload transfer. It does
not optimize the working Discord14/YouTube11 strategies or alter network settings.

Verification: 9 focused Rust tests and 14 frontend/store tests passed; TypeScript
and production frontend build passed. A separate opt-in live smoke diagnostic
uses the same probe without starting/stopping runtime processes:

`cargo test --manifest-path src-tauri/Cargo.toml --lib live_media_probe_without_runtime_changes -- --ignored --nocapture`

Observed on the user's existing connection, without changing the running bypass:

- Rust probe: Discord avatar HTTP 200, 1268 bytes, 194 ms, one attempt.
- Rust probe: ytimg connection timeout, two attempts, 8287 ms, no HTTP response.
- Separate curl checks: Discord avatar 200 / 1268 bytes / 0.91 s; same ytimg image
  200 / 15921 bytes / 3.02 s.

This is evidence of differing client/connection outcomes, not proof of the exact
cause (TLS fingerprint, route/address choice, intermittent network behavior, etc.).
Do not declare YouTube unavailable solely from the Rust probe or silently treat a
timeout as a successful media test. Further transport comparison remains separate.
