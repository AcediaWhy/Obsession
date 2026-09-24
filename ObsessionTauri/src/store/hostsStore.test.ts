import { beforeEach, describe, expect, it, vi } from "vitest";

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    hostsStatus: vi.fn(),
    hostsInstall: vi.fn(),
    hostsRefreshGemini: vi.fn(),
    hostsCheck: vi.fn(),
    hostsUninstall: vi.fn(),
    hostsRestore: vi.fn(),
  },
}));

vi.mock("../lib/tauri", () => ({ api: apiMock }));
vi.mock("./toastStore", () => ({
  toast: { success: vi.fn() },
}));

import type { HostsHealthSnapshot } from "../lib/tauri";
import { useHostsStore } from "./hostsStore";

const health: HostsHealthSnapshot = {
  preferredProvider: "malw",
  installed: true,
  checkedAtUnix: 1_786_816_000,
  repairRecommended: false,
  services: [
    {
      service: "chatgpt",
      health: "working",
      route: "preferred",
      provider: "malw",
      reason: null,
    },
    {
      service: "claude",
      health: "working",
      route: "preferred",
      provider: "malw",
      reason: null,
    },
    {
      service: "gemini",
      health: "working",
      route: "fallback",
      provider: "geohide",
      reason: null,
    },
  ],
};

describe("hosts route health store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useHostsStore.setState({
      revision: -1,
      provider: "malw",
      status: "not_installed",
      localVersion: "",
      remoteVersion: "",
      busy: false,
      error: "",
      rollbackAvailable: false,
      health: null,
    });
  });

  it("coalesces startup checks and forwards the service-owned TTL", async () => {
    useHostsStore.setState({ status: "installed" });
    let resolve!: (value: HostsHealthSnapshot) => void;
    apiMock.hostsCheck.mockReturnValue(
      new Promise<HostsHealthSnapshot>((done) => {
        resolve = done;
      }),
    );

    const first = useHostsStore.getState().checkRoutes(900);
    const second = useHostsStore.getState().checkRoutes(900);
    expect(apiMock.hostsCheck).toHaveBeenCalledTimes(1);
    expect(apiMock.hostsCheck).toHaveBeenCalledWith(900);
    resolve(health);
    await Promise.all([first, second]);
    expect(useHostsStore.getState().health?.services[2].provider).toBe(
      "geohide",
    );
  });

  it("skips a background check while the local health is still fresh", async () => {
    vi.spyOn(Date, "now").mockReturnValue(1_786_816_300_000);
    useHostsStore.setState({ health, status: "installed" });

    await useHostsStore.getState().checkRoutes(3600);

    expect(apiMock.hostsCheck).not.toHaveBeenCalled();
  });

  it("publishes the verified health returned by an explicit install", async () => {
    apiMock.hostsInstall.mockResolvedValue(health);
    apiMock.hostsStatus.mockResolvedValue({
      provider: "malw",
      status: "installed",
      local_version: "2026-08-15",
      remote_version: "",
      rollback_available: true,
    });

    await useHostsStore.getState().install();
    expect(apiMock.hostsInstall).toHaveBeenCalledWith("malw");
    expect(useHostsStore.getState().status).toBe("installed");
    expect(useHostsStore.getState().health).toEqual(health);
  });

  it("keeps the last result when a read-only check itself fails", async () => {
    useHostsStore.setState({ health, status: "installed" });
    apiMock.hostsCheck.mockRejectedValue(
      new Error("Windows runtime-client error: All pipe instances are busy"),
    );

    await useHostsStore.getState().checkRoutes(0);

    expect(useHostsStore.getState().health).toEqual(health);
    expect(useHostsStore.getState().error).toContain("pipe instances are busy");
  });

  it("refreshes only Gemini through the dedicated command and coalesces clicks", async () => {
    let finish!: (value: HostsHealthSnapshot) => void;
    apiMock.hostsRefreshGemini.mockReturnValue(new Promise<HostsHealthSnapshot>((resolve) => { finish = resolve; }));
    apiMock.hostsStatus.mockResolvedValue({ provider: "malw", status: "installed", local_version: "", remote_version: "", rollback_available: true });
    useHostsStore.setState({ health, status: "installed" });
    const first = useHostsStore.getState().refreshGemini();
    await useHostsStore.getState().refreshGemini();
    expect(apiMock.hostsRefreshGemini).toHaveBeenCalledTimes(1);
    expect(apiMock.hostsInstall).not.toHaveBeenCalled();
    expect(useHostsStore.getState().busy).toBe(true);
    finish(health);
    await first;
    expect(useHostsStore.getState().busy).toBe(false);
    expect(useHostsStore.getState().health).toEqual(health);
  });

  it("preserves the previous health when the Gemini repair rolls back", async () => {
    useHostsStore.setState({ health, status: "installed" });
    apiMock.hostsRefreshGemini.mockRejectedValue(new Error("Route check failed; rolled back"));
    await useHostsStore.getState().refreshGemini();
    expect(useHostsStore.getState().health).toEqual(health);
    expect(useHostsStore.getState().busy).toBe(false);
    expect(useHostsStore.getState().error).toContain("rolled back");
  });
});
