import { afterEach, describe, expect, it, vi } from 'vitest';
import { alchemistBitmapSize, closeAlchemistBitmaps, loadAlchemistBitmaps } from './alchemistBitmaps';

afterEach(() => vi.unstubAllGlobals());

describe('bounded alchemist image lifetime', () => {
  it('scales with the display without exceeding source resolution', () => {
    expect(alchemistBitmapSize(240, 1)).toBe(240);
    expect(alchemistBitmapSize(240, 2)).toBe(480);
    expect(alchemistBitmapSize(1000, 4)).toBe(1254);
  });

  it('retains only resized bitmaps and releases every layer', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, blob: async () => new Blob() })));
    const decode = vi.fn(async () => ({ close: vi.fn() }));
    vi.stubGlobal('createImageBitmap', decode);
    const draw = vi.fn();
    const surfaces: { width: number; height: number }[] = [];
    vi.stubGlobal('OffscreenCanvas', class {
      constructor(public width: number, public height: number) { surfaces.push(this); }
      getContext() { return { imageSmoothingEnabled: true, drawImage: draw }; }
      transferToImageBitmap() { return { close: vi.fn() }; }
    });
    const images = await loadAlchemistBitmaps('/assets/', 256, new AbortController().signal);
    expect(decode).toHaveBeenCalledTimes(8);
    expect(draw).toHaveBeenCalledWith(expect.anything(), 0, 0, 256, 256);
    expect(surfaces.every(s => s.width === 0 && s.height === 0)).toBe(true);
    for (const result of decode.mock.results) expect((await result.value).close).toHaveBeenCalledOnce();
    closeAlchemistBitmaps(images);
    Object.values(images).forEach(image => expect(image.close).toHaveBeenCalledOnce());
  });

  it('closes a decode that completes after the theme has unmounted', async () => {
    const abort = new AbortController();
    const close = vi.fn();
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, blob: async () => new Blob() })));
    vi.stubGlobal('createImageBitmap', vi.fn(async () => { abort.abort(); return { close }; }));
    await expect(loadAlchemistBitmaps('/assets/', 256, abort.signal)).rejects.toThrow();
    expect(close).toHaveBeenCalledOnce();
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it('releases prior layers when a later resource fails', async () => {
    const close = vi.fn();
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce({ ok: true, blob: async () => new Blob() })
      .mockResolvedValue({ ok: false, status: 404 }));
    vi.stubGlobal('createImageBitmap', vi.fn(async () => ({ close: vi.fn() })));
    vi.stubGlobal('OffscreenCanvas', class {
      getContext() { return { drawImage: vi.fn() }; }
      transferToImageBitmap() { return { close }; }
    });
    await expect(loadAlchemistBitmaps('/assets/', 256, new AbortController().signal)).rejects.toThrow('404');
    expect(close).toHaveBeenCalledOnce();
  });
});
