import { describe, expect, it } from "vitest";

import {
  applyProgressEvent,
  escapeHtml,
  initialProgressState,
  installerFailureCopy,
  normalizeInstallerFailure,
} from "./installerState";

describe("installer progress sequencing", () => {
  it("accepts only newer, valid and forward-moving events", () => {
    const installing = applyProgressEvent(initialProgressState, {
      sequence: 2,
      pct: 32,
      stage: "install",
    });
    expect(installing).toEqual({ sequence: 2, pct: 32, stage: "install" });
    expect(applyProgressEvent(installing, { sequence: 1, pct: 90, stage: "verify" })).toBe(installing);
    expect(applyProgressEvent(installing, { sequence: 3, pct: 12, stage: "stage" })).toBe(installing);
    expect(applyProgressEvent(installing, { sequence: 3, pct: 101, stage: "verify" })).toBe(installing);
  });

  it("allows the explicit rollback stage without accepting stale events", () => {
    const committed = { sequence: 10, pct: 84, stage: "registry" };
    expect(applyProgressEvent(committed, { sequence: 11, pct: 86, stage: "rollback" })).toEqual({
      sequence: 11,
      pct: 86,
      stage: "rollback",
    });
    expect(applyProgressEvent(committed, { sequence: 10, pct: 86, stage: "rollback" })).toBe(committed);
  });
});

describe("installer failures", () => {
  it("keeps structured failures and hides legacy raw errors", () => {
    const structured = {
      code: "UAC_CANCELLED" as const,
      retryable: true,
      messageCode: "installer.error.uac_cancelled",
      logPath: "C:\\installer.log",
    };
    expect(normalizeInstallerFailure(structured, "INSTALL_FAILED")).toEqual(structured);
    expect(normalizeInstallerFailure("Access denied <script>", "PREFLIGHT_FAILED")).toEqual({
      code: "PREFLIGHT_FAILED",
      retryable: true,
      messageCode: "installer.error.preflight_failed",
      logPath: null,
    });
    expect(installerFailureCopy("ROLLBACK_INCOMPLETE").action).toBe("");
  });

  it("defines localized copy and retry policy for every public error code", () => {
    const cases = [
      ["BUSY", true],
      ["UAC_CANCELLED", true],
      ["DOWNGRADE_BLOCKED", false],
      ["PREFLIGHT_FAILED", true],
      ["INSTALL_FAILED", true],
      ["ROLLBACK_RESTORED", true],
      ["ROLLBACK_INCOMPLETE", false],
      ["LAUNCH_FAILED", true],
    ] as const;
    for (const [code, retryable] of cases) {
      const failure = normalizeInstallerFailure(null, code);
      expect(failure.retryable).toBe(retryable);
      expect(installerFailureCopy(code).title.length).toBeGreaterThan(4);
      expect(installerFailureCopy(code).summary.length).toBeGreaterThan(12);
    }
  });

  it("escapes dynamic text before it enters installer templates", () => {
    expect(escapeHtml('<img src=x onerror="boom">')).toBe(
      "&lt;img src=x onerror=&quot;boom&quot;&gt;",
    );
  });
});
