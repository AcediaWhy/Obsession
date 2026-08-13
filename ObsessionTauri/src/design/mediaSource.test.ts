import { describe, expect, it, vi } from "vitest";

import { releaseMediaSource, restoreMediaSource, type MediaSourceTarget } from "./mediaSource";

function createMediaSource(src: string) {
  let currentSource: string | null = src;
  const media: MediaSourceTarget = {
    getAttribute: vi.fn(() => currentSource),
    setAttribute: vi.fn((_name, value) => {
      currentSource = value;
    }),
    removeAttribute: vi.fn(() => {
      currentSource = null;
    }),
    load: vi.fn(),
    pause: vi.fn(),
  };

  return { media, source: () => currentSource };
}

describe("media source lifecycle", () => {
  it("restores the source after a StrictMode cleanup replay", () => {
    const src = "/themes/catnap.webm";
    const { media, source } = createMediaSource(src);

    restoreMediaSource(media, src);
    expect(media.load).not.toHaveBeenCalled();

    releaseMediaSource(media);
    expect(source()).toBeNull();
    expect(media.pause).toHaveBeenCalledOnce();
    expect(media.load).toHaveBeenCalledOnce();

    restoreMediaSource(media, src);
    expect(source()).toBe(src);
    expect(media.setAttribute).toHaveBeenCalledWith("src", src);
    expect(media.load).toHaveBeenCalledTimes(2);
  });
});
