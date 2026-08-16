import { describe, expect, it } from "vitest";

import { resolveTelegramQrLink } from "./telegramQrLink";

describe("resolveTelegramQrLink", () => {
  it("keeps the native Telegram proxy scheme for direct app handoff", () => {
    const link = "tg://proxy?server=192.168.1.5&port=1443&secret=dd00";

    expect(resolveTelegramQrLink(link)).toBe(link);
  });

  it("preserves a backend-provided fallback and an absent link", () => {
    const fallback = "https://t.me/proxy?server=192.168.1.5&port=1443&secret=dd00";

    expect(resolveTelegramQrLink(fallback)).toBe(fallback);
    expect(resolveTelegramQrLink(null)).toBeNull();
  });
});
