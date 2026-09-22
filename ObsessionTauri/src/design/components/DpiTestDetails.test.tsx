import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { DpiTestDetails, testSummary } from "./DpiTestDetails";
import type { DpiTestReport } from "../../lib/tauri";

describe("DPI diagnostic evidence", () => {
  const report: DpiTestReport = {passed: false, status: "partial", checks: [{
    url: "https://i.ytimg.com/vi/example/hqdefault.jpg", passed: false,
    elapsed_ms: 7250, bytes: 0, attempts: 2, http_status: null, error: "connection reset",
  }]};
  it("does not label partial service access as a broken config", () => {
    expect(testSummary(report)).toBe("частично");
    expect(testSummary({...report, passed:true, status:"passed"})).toBe("проверки пройдены");
  });
  it("shows target, timing, retries and actual error without claiming video verification", () => {
    const html = renderToStaticMarkup(<DpiTestDetails report={report} />);
    for (const value of ["Превью YouTube", "7.25", "connection reset", "попыток: 2", "не проверяются"]) {
      expect(html).toContain(value);
    }
  });
});
