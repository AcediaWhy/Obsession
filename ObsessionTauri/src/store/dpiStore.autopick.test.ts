import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("../lib/tauri", () => ({ api: {
  dpiTest: vi.fn(), recordWorkingConfig: vi.fn().mockResolvedValue(undefined),
  updateSettings: vi.fn().mockResolvedValue({}), dpiTestCancel: vi.fn().mockResolvedValue(undefined),
}, runtime: {} }));
import { api, type AppConfig, type DpiTestReport } from "../lib/tauri";
import { useDpiStore } from "./dpiStore";
const result = (passed: boolean): DpiTestReport => ({passed, status: passed ? "passed" : "failed", checks: []});
beforeEach(() => {
  vi.clearAllMocks();
  useDpiStore.setState({ active:false, transitioning:false, testing:false, testCancel:false,
    selectedCategories:["discord"], selectedConfigs:{discord:"old.conf"},
    config:{configs:{discord:["one.conf","two.conf"]}} as unknown as AppConfig,
    loadStats:vi.fn().mockResolvedValue(undefined), error:"", engines:[], netStats:{} });
});
describe("autopick outcomes", () => {
  it("reports exhaustion without silently blessing the previous config", async () => {
    vi.mocked(api.dpiTest).mockResolvedValue(result(false));
    await useDpiStore.getState().autoConfigure();
    expect(useDpiStore.getState().selectedConfigs.discord).toBe("old.conf");
    expect(useDpiStore.getState().error).toContain("прошедший все проверки");
    expect(api.recordWorkingConfig).not.toHaveBeenCalled();
    expect(useDpiStore.getState().testing).toBe(false);
  });
  it("stops on a service error and releases the testing latch", async () => {
    vi.mocked(api.dpiTest).mockRejectedValue(new Error("service unavailable"));
    await useDpiStore.getState().autoConfigure();
    expect(api.dpiTest).toHaveBeenCalledTimes(1);
    expect(useDpiStore.getState().error).toContain("service unavailable");
    expect(useDpiStore.getState().testing).toBe(false);
  });
  it("does not save an in-flight success after cancel", async () => {
    vi.mocked(api.dpiTest).mockImplementation(async () => {
      useDpiStore.setState({testCancel:true}); return result(true);
    });
    await useDpiStore.getState().autoConfigure();
    expect(api.recordWorkingConfig).not.toHaveBeenCalled();
    expect(useDpiStore.getState().selectedConfigs.discord).toBe("old.conf");
  });
  it("skips failed candidates and publishes each result", async () => {
    vi.mocked(api.dpiTest).mockResolvedValueOnce(result(false)).mockResolvedValueOnce(result(true));
    await useDpiStore.getState().autoConfigure();
    expect(useDpiStore.getState().selectedConfigs.discord).toBe("two.conf");
    expect(useDpiStore.getState().testResults).toEqual({"one.conf":false,"two.conf":true});
  });
  it("keeps the user's selection and exposes partial evidence without recording success", async () => {
    vi.mocked(api.dpiTest).mockResolvedValue({passed:false, status:"partial", checks:[]});
    await useDpiStore.getState().autoConfigure();
    expect(useDpiStore.getState().selectedConfigs.discord).toBe("old.conf");
    expect(useDpiStore.getState().testReports["one.conf"].status).toBe("partial");
    expect(api.recordWorkingConfig).not.toHaveBeenCalled();
  });
  it("manual testing retains a partial report", async () => {
    vi.mocked(api.dpiTest).mockResolvedValue({passed:false, status:"partial", checks:[]});
    await useDpiStore.getState().testAll();
    expect(useDpiStore.getState().testReports["old.conf"].status).toBe("partial");
    expect(useDpiStore.getState().testing).toBe(false);
    expect(api.recordWorkingConfig).not.toHaveBeenCalled();
  });
  it("retests the network's last working config before other candidates", async () => {
    useDpiStore.setState({
      selectedConfigs: {discord:"one.conf"},
      netStats: {discord:{conf:"two.conf", success_count:5, confirmed_at:123}},
    });
    vi.mocked(api.dpiTest).mockResolvedValue(result(true));
    await useDpiStore.getState().autoConfigure();
    expect(api.dpiTest).toHaveBeenCalledTimes(1);
    expect(api.dpiTest).toHaveBeenCalledWith("discord", "two.conf", true);
    expect(api.recordWorkingConfig).toHaveBeenCalledWith("discord", "two.conf");
  });
  it("tries current config next, ignores absent cached files, and keeps candidates unique", async () => {
    useDpiStore.setState({
      selectedConfigs: {discord:"two.conf"},
      netStats: {discord:{conf:"removed.conf", success_count:5, confirmed_at:123}},
    });
    vi.mocked(api.dpiTest).mockResolvedValue(result(false));
    await useDpiStore.getState().autoConfigure();
    expect(vi.mocked(api.dpiTest).mock.calls).toEqual([
      ["discord", "two.conf", true], ["discord", "one.conf", true],
    ]);
    expect(useDpiStore.getState().selectedConfigs.discord).toBe("two.conf");
  });
  it("manual testing requests a complete report rather than stopping at the first failure", async () => {
    vi.mocked(api.dpiTest).mockResolvedValue(result(false));
    await useDpiStore.getState().testAll();
    expect(api.dpiTest).toHaveBeenCalledTimes(1);
    expect(api.dpiTest).toHaveBeenCalledWith("discord", "old.conf");
  });
});
