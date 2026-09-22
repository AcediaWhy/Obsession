# Legacy TCP timestamp prerequisite

The controlled test on this machine confirmed that Discord gateway/updater TLS
failed with global TCP timestamps `allowed`, succeeded with `enabled`, and failed
again after restoring `allowed`, without changing the active Legacy strategies.
`discord_12.conf` uses timestamp fooling; `allowed` does not negotiate timestamps
on outgoing TCP connections.

## Service behavior

- Inspect the protected, materialized Legacy response file for exact `ts` in
  `--dpi-desync-fooling` or `--dup-fooling`. Zapret2 does not request this lease.
- Save the original `allowed`/`disabled` value in the ACL-protected
  `%ProgramData%\Obsession\Runtime\tcp-settings\timestamps.restore` before changing
  anything. Flush the file before running the fixed system `netsh.exe` command.
- Set only `interface tcp set global timestamps=enabled`, verify the result, then
  launch the DPI job. No TCP reset or other global settings are applied.
- Stop/close the job before restoring the original setting. A failed launch also
  drops the job before the lease. A setting that was already enabled is untouched.
- Keep the journal on restoration failure. Retry before another launch and at
  service startup, including startup after a crash. This is **next-start recovery**,
  not an independent watchdog while the service is down.
- Preserve an external change to `allowed` or `disabled` during the session.
  An external change to the same `enabled` value is indistinguishable from our own
  change and will be restored to the saved original value.
- Corrupt journals fail closed; they are not guessed or silently discarded.

The setting is machine-wide while the lease is active, not per-application.
Existing TCP connections are not renegotiated; restart Discord after enabling the
new runtime when validating the fix. Do not delete the journal to clear a recovery
error. Restart the service and resolve any restore failure before uninstalling.

## Verification

- 11 focused tests: strict dump/config parsing, the bundled Discord12 strategy,
  durable file roundtrip/corruption, rollback, startup recovery, no-op cases,
  external changes, duplicate acquisition, and failed-restore retries.
- Broader service run: 150 unit tests plus the combined Legacy lifecycle test
  passed. Seven named-pipe tests excluded from that sandbox run.
- 276 protected engine `--dry-run` invocations passed; these do not capture traffic.
- Real Discord behavior with the newly installed service still requires validation.

References:
- https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/netsh-interface
- https://github.com/bol-van/zapret/blob/master/docs/readme.en.md
