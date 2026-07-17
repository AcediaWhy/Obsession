import { describe, it, expect, vi, beforeEach } from "vitest";

// Сторы импортируют api/on/clipboard из ../lib/tauri (который тянет Tauri-рантайм,
// недоступный в node). Мокаем весь модуль — редьюсеры от него не зависят.
vi.mock("../lib/tauri", () => ({
  api: {},
  clipboard: { write: vi.fn() },
}));

import { useProxyStore } from "./proxyStore";
import type { ProxyStatus } from "../lib/tauri";

const baseStatus: ProxyStatus = {
  running: true,
  link: "tg://proxy?server=127.0.0.1&port=1443&secret=dd00",
  lan_link: "tg://proxy?server=192.168.1.5&port=1443&secret=dd00",
  lan_published: true,
  lan_expiry_unix: 1_700_000_000,
};

describe("proxyStore.applyStatus", () => {
  beforeEach(() => {
    useProxyStore.setState({
      revision: -1,
      running: false,
      link: "",
      lanLink: null,
      lanPublished: false,
      lanExpiryUnix: null,
      transitioning: false,
    });
  });

  it("применяет поля статуса из payload", () => {
    useProxyStore.getState().applyStatus(baseStatus);
    const s = useProxyStore.getState();
    expect(s.running).toBe(true);
    expect(s.link).toBe(baseStatus.link);
    expect(s.lanLink).toBe(baseStatus.lan_link);
    expect(s.lanPublished).toBe(true);
    expect(s.lanExpiryUnix).toBe(baseStatus.lan_expiry_unix);
  });

  it("НЕ трогает transitioning — им владеют start/stop (гонка даблклика)", () => {
    // Имитируем идущий start(): латч взведён.
    useProxyStore.setState({ transitioning: true });
    // Ранний proxy-status во время операции не должен снять латч.
    useProxyStore.getState().applyStatus(baseStatus);
    expect(useProxyStore.getState().transitioning).toBe(true);
  });

  it("сравнивает proxy revision независимо от остальных подсистем", () => {
    expect(
      useProxyStore.getState().applyVersionedStatus({
        revision: 4,
        value: baseStatus,
      }),
    ).toBe(true);
    expect(
      useProxyStore.getState().applyVersionedStatus({
        revision: 3,
        value: { ...baseStatus, running: false },
      }),
    ).toBe(false);
    expect(useProxyStore.getState().revision).toBe(4);
    expect(useProxyStore.getState().running).toBe(true);
  });
});
