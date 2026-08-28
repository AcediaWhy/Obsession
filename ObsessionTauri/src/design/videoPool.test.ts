import { afterEach, describe, expect, it, vi } from "vitest";

import { pooledVideo, resetVideoPool, videoPoolSize, type PooledVideo } from "./videoPool";

function createFake(): PooledVideo {
  let source: string | null = null;
  return {
    poster: "",
    autoplay: false,
    loop: false,
    muted: false,
    playsInline: false,
    preload: "",
    className: "",
    getAttribute: vi.fn(() => source),
    setAttribute: vi.fn((_name, value) => {
      source = value;
    }),
    removeAttribute: vi.fn(() => {
      source = null;
    }),
    load: vi.fn(),
    pause: vi.fn(),
    remove: vi.fn(),
  };
}

afterEach(() => {
  resetVideoPool();
});

describe("videoPool", () => {
  it("reuses one element per source across mounts", () => {
    const create = vi.fn(createFake);
    const first = pooledVideo("/catnap/loop.mp4", "/catnap/poster.jpg", create);
    const second = pooledVideo("/catnap/loop.mp4", "/catnap/poster.jpg", create);

    expect(second).toBe(first);
    expect(create).toHaveBeenCalledOnce();
    // Повторный заход не перезапускает загрузку: конвейер остаётся тем же.
    expect(first.load).toHaveBeenCalledOnce();
  });

  it("configures a new element for a silent looping background", () => {
    const video = pooledVideo("/midnight/loop.mp4", "/midnight/poster.jpg", createFake);

    expect(video.autoplay).toBe(true);
    expect(video.loop).toBe(true);
    expect(video.muted).toBe(true);
    expect(video.playsInline).toBe(true);
    expect(video.preload).toBe("auto");
    expect(video.poster).toBe("/midnight/poster.jpg");
    expect(video.setAttribute).toHaveBeenCalledWith("src", "/midnight/loop.mp4");
  });

  it("caps the pool at the number of video themes, not the number of switches", () => {
    const create = vi.fn(createFake);
    for (let round = 0; round < 30; round += 1) {
      pooledVideo("/catnap/loop.mp4", "/catnap/poster.jpg", create);
      pooledVideo("/midnight/loop.mp4", "/midnight/poster.jpg", create);
    }

    expect(videoPoolSize()).toBe(2);
    expect(create).toHaveBeenCalledTimes(2);
  });

  it("releases sources and detaches elements on reset", () => {
    const video = pooledVideo("/catnap/loop.mp4", undefined, createFake);
    resetVideoPool();

    expect(video.pause).toHaveBeenCalled();
    expect(video.removeAttribute).toHaveBeenCalledWith("src");
    expect(video.remove).toHaveBeenCalledOnce();
    expect(videoPoolSize()).toBe(0);
  });
});
