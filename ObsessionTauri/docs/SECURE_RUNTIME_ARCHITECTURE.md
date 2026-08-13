# Secure Runtime Architecture

Status: implementation contract for the post-hotfix Windows runtime.

This document supersedes the elevation sections of
`ONBOARDING_OVERHAUL_SPEC.md`. Obsession must never solve a privileged action by
relaunching the Tauri UI as administrator.

## 1. Threat model

Assume an attacker can already execute code as the interactive Windows user.
That attacker can:

- replace any file below the user's `%LOCALAPPDATA%`, `%APPDATA%`, downloads and
  temporary directories;
- call exposed Tauri commands and connect to user-accessible IPC endpoints;
- inspect command lines and modify user-owned settings, lists and registry keys;
- race reads of files stored in user-writable directories.

The attacker cannot write to correctly ACLed `%ProgramFiles%` or `%ProgramData%`
locations and cannot obtain an elevated token without a UAC decision or another
vulnerability.

The security goal is not to hide privileged IPC from the same user. The service
must remain safe even when every allowed request is issued by hostile code under
that user's account.

## 2. Non-negotiable invariants

1. `Obsession.exe` always runs as the interactive user at medium integrity.
2. A privileged process never executes or loads EXE, DLL, SYS, Lua, config or
   command-line fragments from a user-writable location.
3. Privileged IPC contains no arbitrary executable path, working directory,
   shell command, environment block or raw argument vector.
4. Every privileged operation is an allowlisted, versioned request with bounded
   fields and server-side validation.
5. User-supplied domain/IP data is treated only as data. It is revalidated and
   copied into a service-owned `%ProgramData%` file before a runtime process can
   consume it; a validated user path is never passed through because that would
   leave a TOCTOU race.
6. DPI processes are owned by a Windows Job Object and stopped by exact owned
   handles/PIDs. Global `taskkill /IM` is not part of the new runtime.
7. The installer, application, service and runtime payloads are code-signed for
   release. The service also verifies the protected resource manifest before
   use.
8. If the service, protocol version, protected resources or ACLs cannot be
   verified, the client remains fail-closed. There is no fallback to the old
   AppData runtime.

## 3. Components and storage

### Medium-integrity client

`%ProgramFiles%\Obsession\Obsession.exe`

- owns the Tauri UI, per-user settings, diagnostics and orchestration UI;
- never opens WinDivert, writes the system hosts file or starts `winws`;
- may run `TgWsProxy.exe` at medium integrity from the protected installation;
- asks the service only for the narrow privileged portion of an operation.

### Privileged Windows service

Service name: `ObsessionRuntime`

Binary: `%ProgramFiles%\Obsession\runtime\Obsession.Runtime.exe`

- runs as `LocalSystem` and exposes a local named-pipe RPC endpoint;
- owns `winws`/`winws2`, WinDivert capture (Eyes), driver lifecycle and exact
  process teardown;
- owns transactional system-hosts mutation and the Obsession firewall rules;
- keeps no general-purpose process, filesystem or shell RPC primitive.

The service host registers the fixed SCM identity, reports start/running/stop
states, accepts only stop/shutdown controls and terminates an idle pipe listener
within a bounded polling interval. Before reporting `RUNNING`, it performs the
complete protected install/state preflight and constructs the production DPI
backend. If any check fails, that service process stays queryable through
`LockedBackend` with an empty capability list and never retries into a
privileged capability. The setup binary contains a strict internal
`--obsession-machine-worker-v1 provision <request-id>` mode which performs
fixed-path payload materialization, native ProgramData ACL provisioning and
idempotent SCM create/update/start work before any WebView is initialized. The
request ID is exactly 32 lowercase hexadecimal characters and can select only a
fixed result filename below the protected Runtime state directory. The worker
accepts no caller path, service name or command text and refuses a non-elevated
token.

The medium setup now invokes this mode through `ShellExecuteExW` with the fixed
`runas` verb. It holds its own executable open without write/delete sharing from
UI startup through the UAC handoff, waits on the exact returned process handle
with a ten-minute bound, and accepts success only when both the exit code and a
strict protected result report agree. Progress reports are bounded JSON written
with create-new semantics below `%ProgramData%\Obsession\Runtime`; partial
writes are ignored while the child is running and rejected after exit. Each
worker run also performs a bounded scan and retains at most 32 recent result
files (plus the active request), deleting entries older than 14 days.

The release build now prepares the protected payload before building the setup
wrapper. It compiles `Obsession.Runtime.exe`, generates a deterministic strict
manifest with SHA-256 and size entries, includes every Legacy engine/DLL/driver,
config and referenced dependency, and runs every bundled Legacy strategy
through the real catalog resolver and materializer in a synthetic
`Program Files\Obsession` layout. The outer setup also embeds that exact installed
layout as a separate raw `OBSMACH1` machine payload. Its bounded manifest fixes
the product/version identity, file count, ASCII relative paths, sizes and
SHA-256 hashes; traversal, reserved Windows names, case-insensitive duplicates,
missing required binaries and unauthenticated trailing bytes are rejected.

For an initial machine install, the native worker creates a protected sibling
staging directory below Program Files, writes every embedded file with
create-new semantics, flushes and re-hashes the result, revalidates the runtime
service manifest, then renames the stage to `%ProgramFiles%\Obsession` and
verifies the committed tree again. Reparse points are rejected before bounded
failure cleanup. Existing installs are compared against the complete embedded
tree, including rejection of unauthenticated extra files. A mismatch enters a
native transactional update: the worker verifies the old protected catalog,
prepares an exact protected sibling stage, writes a two-slot durable journal in
`%ProgramData%`, stops the service, swaps target/backup directories with
write-through renames, verifies and starts the new service, then commits and
cleans the backup. Every pre-commit failure rolls the directory and service back
to the verified old installation. Interrupted operations are recovered from
the journal on the next run, and a global machine mutex prevents two sessions
from mutating the shared installation concurrently. The wrapper no longer
embeds, extracts or launches a current-user NSIS payload. Its normal WebView
flow and its no-WebView fallback both use the same native UAC worker.

The protected Legacy executor is connected to the SCM transport. It resolves
only manifest-selected configs, rewrites every resource reference to a verified
Program Files path, materializes mutable `--hostlist-auto` state below the fixed
ProgramData root, and re-hashes both protected and generated files immediately
before launch. Processes are created suspended, assigned to an unnamed
kill-on-close Job Object with an exact active-process limit, identified by PID +
creation time + executable SHA-256, and only then resumed. Readiness and stop
waits are bounded; generation/fingerprint mismatches fail closed, and generation
cleanup rejects reparse points before recursive removal.

`Feature::Dpi` is advertised only after this startup preflight succeeds. The
backend accepts Legacy response-file plans and, when the protected manifest
contains the verified built-in pack, advertises `Feature::DpiZapret2`. Zapret2
profiles are selected by typed category-group IDs; the service verifies the
pack/Lua/blob/list hashes and compiles the complete argv itself. No raw IPC
arguments, user paths or caller-controlled Lua/config content cross the
privileged boundary. Hosts, Firewall and events remain unavailable.

Production discovery also verifies `%ProgramData%\Obsession` and
`%ProgramData%\Obsession\Runtime` on startup and again before materialization.
Both directories must be owned by LocalSystem or Builtin Administrators, use a
protected non-null DACL, contain only understood allow/deny ACE forms, and grant
write-like directory rights only to LocalSystem or Administrators. The
test/installer `inspect()` seam does not relax production discovery; it merely
allows isolated fixtures to exercise materialization before installer ACL work
is connected. Every existing or newly created DPI subdirectory and materialized
file is checked against the same owner/write policy before use, so a weak child
DACL left by an older installation cannot reopen the protected root.

### Storage boundaries

| Location | Owner/use | May contain code-bearing files |
| --- | --- | --- |
| `%ProgramFiles%\Obsession` | installer + service, users read/execute | yes |
| `%ProgramData%\Obsession\Runtime` | service state, sanitized runtime data, journals | no executable code; service-written data only |
| `%LOCALAPPDATA%\vlarpsu\Obsession` | per-user settings, profiles, UI logs/cache | no privileged runtime code |

The current mixed `Paths::base_dir` model must be split into immutable install
resources and mutable user data before the service gate can become available.

The service trusts only
`%ProgramFiles%\Obsession\runtime\runtime-manifest.json`. This is a separate,
strict manifest rather than the legacy app `manifest.json`: unknown fields are
rejected, its size and collection counts are bounded, and every executable,
config, Lua file, list and strategy dependency must have an exact size and
SHA-256 entry. Strategy records contain typed category/ID references only; IPC
paths can never be substituted into the resolved catalog.

## 4. IPC contract

Transport: local Windows named pipe with a service-created DACL. The server must
obtain the client PID/session/token from the pipe and reject remote clients,
service/session-zero callers and unsupported integrity/token states.

The well-known pipe name is not an identity. The service creates only the first
pipe instance and fails closed if another process already owns that name. The
medium client verifies the connected server PID is a session-zero LocalSystem
process whose image is exactly
`%ProgramFiles%\Obsession\runtime\Obsession.Runtime.exe`, with no reparse point
in the protected path, before it sends request bytes. The client opens the pipe
with `SECURITY_IDENTIFICATION`, allowing the service to inspect its token while
preventing a spoofed server from impersonating it for resource access.

After a client connects, all client-controlled reads, writes and response flush
run on a scoped worker with one total I/O deadline. On expiry the host cancels
the worker's synchronous pipe I/O and disconnects that instance, so a client
that sends a partial frame or never reads its response cannot pin the service
forever. The SCM stop handler separately cancels the idle listener and dropping
the backend closes the Job Object for any active DPI generation.

Messages are length-prefixed, versioned and capped (initial maximum: 64 KiB).
All request structures use strict deserialization (`deny_unknown_fields`) and
bounded strings/collections. Each response echoes a request ID and carries a
stable typed error code.

Initial allowlist:

```text
GetCapabilities
GetRuntimeSnapshot
DpiStart { engine, selections, runtime_options }
DpiStop { generation }
HostsInstall { provider }
HostsUninstall
HostsRestoreLastKnownGood { provider }
FirewallOpenProxyLan { port, lease_seconds }
FirewallCloseProxyLan
SubscribeEvents
```

Notably absent:

- `Run(path, args)`;
- `LoadDll(path)`;
- `WriteFile(path, bytes)`;
- arbitrary hosts content or firewall command text;
- arbitrary config/list paths;
- PowerShell/cmd execution.

Engine names, categories, strategy IDs and providers are enums. The service maps
them to files below its own protected install root. Mutable lists are sent as
bounded typed entries, revalidated by the service and atomically materialized
below `%ProgramData%`.

Authentication does not turn hostile same-user code into a trusted client.
Pipe ACLs, client-token checks and per-session ownership reduce cross-user
interference, but privilege safety comes from the request allowlist itself.

## 5. Runtime ownership

- Only one machine-wide DPI runtime may own WinDivert at a time.
- `DpiStart` returns a non-zero generation and a service-created runtime ID.
- All spawned processes are assigned to a kill-on-close Job Object before the
  runtime is reported ready.
- The service records the exact executable identity, process handle, creation
  time and generation. PID-only ownership is insufficient.
- Eyes runs inside the service and emits sanitized observations/status events;
  raw packets never cross IPC.
- Service restart performs bounded recovery from the protected runtime journal
  and never adopts an unverified process merely by image name.

## 6. Telegram proxy split

`TgWsProxy.exe` does not need an elevated token for loopback operation. The
medium client starts the fixed protected binary directly only after its exact
`bin/tg_ws_proxy.exe` size and SHA-256 match the protected runtime manifest. The
service is used only to add/remove the exact Obsession LAN firewall rule when
LAN publication is requested.

The service owns one fixed rule name and group. It creates the rule through the
native Firewall COM API as inbound TCP, `Private` profile and `LocalSubnet`
only. The medium client supplies only a non-zero typed port and a lease duration
bounded by the wire protocol. It must acquire the lease before binding the
`0.0.0.0` forwarder, renew it while publication remains active and close the
forwarder if renewal fails. The service removes expired rules during background
polling, on explicit close, on clean shutdown and during the next startup
preflight after a crash. A same-name rule with any unexpected property is left
untouched and disables the capability fail-closed.

This keeps the proxy process out of the privileged boundary and preserves the
ability to start and stop loopback operation without UAC or firewall capability.

## 7. Per-machine installer and migration

The new installation target is fixed:

`%ProgramFiles%\Obsession`

The elevated install worker performs only machine-wide work:

- transactional Program Files staging/swap;
- protected `%ProgramData%` journal/state creation;
- service create/update/start/stop/delete;
- HKLM uninstall registration;
- all-users Start Menu shortcut and optional Public Desktop shortcut.

It must not write HKCU or resolve `Desktop`/`Programs` for the elevated account.
The original medium-integrity setup process performs current-user HKCU cleanup
and autostart migration after the elevated worker succeeds.

The native worker synchronizes the selected all-users Start Menu shortcut and
optional Public Desktop shortcut from `FOLDERID_CommonPrograms` and
`FOLDERID_PublicDesktop`. The only accepted shortcut options are fixed `0`/`1`
tokens; no caller-supplied path crosses the UAC boundary. Existing foreign or
unreadable shortcuts are preserved, while Obsession shortcuts are identified by
their exact protected target before overwrite or removal. The medium process
removes only owned current-user/legacy shortcuts, records the protected Program
Files path in HKCU, removes the obsolete current-user uninstall record and
repoints an already-enabled HKCU autostart value. It does not enable autostart
for users who had it disabled. Native initial install, transactional update,
HKLM uninstall registration and the fixed protected uninstaller are connected. The setup image
is copied and hash-checked as `%ProgramFiles%\Obsession\uninstall.exe`; Windows
Apps & Features invokes only that path. Its medium entry point performs the UAC
handoff and current-user shortcut/HKCU cleanup, while the elevated fixed worker
stops/deletes the service and removes protected machine files/state. Files locked
by the running uninstaller are scheduled through the native reboot-delete API,
never through a helper copied to `%TEMP%`.

Allowlisted current-user settings/profile migration, legacy AppData executable
quarantine, fail-closed hosts recovery, native firewall cleanup and
all-users/Public shortcut ownership are connected. Real UAC
install/update/uninstall testing remains pending, so the protected-runtime
application gate stays closed. Release signing is a deferred distribution and
publisher-identity improvement rather than a prerequisite for the Program Files
privilege-boundary fix; unsigned artifacts will continue to show an unknown
publisher and may trigger SmartScreen reputation warnings.

The install worker is native code inside the setup executable. The
current temporary PowerShell `-Verb RunAs` helper is removed; no script extracted
to a user-writable temporary directory is ever elevated.

Migration from `%LOCALAPPDATA%\Obsession` follows this order:

1. The medium setup discovers the current user's legacy HKCU records and exact
   legacy path.
2. The elevated worker independently validates that the legacy target is the
   expected Obsession layout and stops verified legacy processes.
3. The protected per-machine application/service is installed and verified.
4. The medium setup migrates only allowlisted user data (settings/profiles),
   updates HKCU autostart to the protected executable and removes obsolete HKCU
   install records.
5. The LocalSystem runtime service derives the authenticated pipe caller's
   profile only through `HKLM\\...\\ProfileList\\<SID>`, stops processes whose
   exact image path is below the two fixed legacy roots, and quarantines/removes
   the executable-bearing Local tree. It never accepts a cleanup path or command
   from the client and never copies EXE/DLL/SYS/Lua/config files into the
   protected installation.
6. Before Roaming can be removed, the service strictly parses bounded schema-v1
   `hosts-state.json`, verifies the byte-exact original snapshot and proves the
   current system file still has the matching Obsession marker and applied hash.
   Automatic restoration accepts only a harmless empty/comment/localhost
   baseline. Custom, corrupt, reparse-backed or externally changed state is
   preserved for manual recovery instead of becoming a privileged write channel.
7. The service enumerates Windows Firewall rules through the native COM API and
   removes only exact historical `Obsession TgWsProxy` rule shapes after checking
   direction, action, protocol, port, group/profile and program/service scope.
   Roaming is quarantined only after hosts and firewall cleanup both succeed.

Failure before step 3 leaves the legacy install untouched but still subject to
the fail-closed application hotfix. Failure after step 3 prefers the protected
install and reports explicit per-user cleanup work; it never launches the old
copy elevated.

## 8. Update and uninstall

- Updates use the setup package and the same elevated native worker. Release
  signing can be added later without changing this privilege boundary.
- The worker authenticates the embedded product/version/payload, verifies the
  existing protected catalog and SCM configuration, stops the service, performs
  a protected sibling stage/backup swap, re-verifies the exact committed tree
  and restarts the service.
- Two-slot transaction journals live in protected `%ProgramData%`, not
  `%LOCALAPPDATA%`; all pre-commit checkpoints have rollback/recovery tests.
- Uninstall is per-machine/elevated: it stops/deletes the service, removes the
  HKLM registration, Program Files payload and protected ProgramData. Legacy
  hosts/firewall ownership is now resolved during per-user migration before its
  Roaming recovery data can be deleted; the new protected hosts/firewall feature
  lifecycle remains disabled until its server-side implementation is complete.
- Current-user HKCU cleanup is done by the medium setup/app. Other users' UI
  data is not recursively deleted by an elevated uninstaller.
- The registered uninstaller is an authenticated copy of the setup image below
  Program Files. It accepts no path-bearing uninstall arguments; locked main,
  uninstaller and result files are deleted at the next reboot.

## 9. Rollout gates

The temporary `protected_runtime_available() == false` gate stays closed until
all of the following are true:

1. service protocol and strict parser tests pass;
2. process/WinDivert/hosts implementations use only protected paths;
3. installer creates verified Program Files/ProgramData ACLs and the service;
4. legacy AppData migration has automated fault-injection tests;
5. update/uninstall stop and recover the service transactionally;
6. release artifacts pass a real Windows UAC install/update/uninstall checklist;
7. a same-user adversarial test confirms that arbitrary paths, arguments,
   scripts, configs and list-file races cannot cross the privileged boundary.

The required real-Windows matrix is:

- fresh install from a standard account, including both UAC acceptance and
  cancellation;
- verification that files land only below Program Files/ProgramData, HKLM is
  machine-wide, HKCU remains the initiating user's, and shortcuts land in
  Common Programs/Public Desktop rather than the administrator profile;
- repair/update while the client and runtime service are active;
- migration from the historical `%LOCALAPPDATA%` install with safe, custom,
  externally modified and malformed `hosts` state plus legacy and
  generation-aware firewall rules;
- uninstall with shortcuts enabled/disabled, an unrelated same-name shortcut,
  locked files requiring reboot deletion, and a second Windows user profile;
- post-operation ACL checks proving that a standard user cannot replace the
  Program Files executable, service binary or machine shortcuts.

Only then may the capability become a dynamic service handshake instead of the
hardcoded fail-closed value.
