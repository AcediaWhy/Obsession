import { exactFiles } from './alchemistExact';

export type AlchemistBitmaps = Record<keyof typeof exactFiles, ImageBitmap>;

// Bound retained decoded pixels to the rendered size, including display scaling.
export function alchemistBitmapSize(width: number, dpr: number): number {
  return Math.min(1254, Math.max(1, Math.round(width * Math.min(dpr || 1, 3))));
}

export function closeAlchemistBitmaps(images: Partial<AlchemistBitmaps>) {
  Object.values(images).forEach(image => image.close());
}

export async function loadAlchemistBitmaps(base: string, pixels: number, signal: AbortSignal): Promise<AlchemistBitmaps> {
  const images: Partial<AlchemistBitmaps> = {};
  try {
    // Decode one source at a time: eight full-size concurrent decodes spike memory.
    for (const [key, filename] of Object.entries(exactFiles)) {
      signal.throwIfAborted();
      const response = await fetch(`${base}${filename}.webp`, { signal });
      if (!response.ok) throw new Error(`Alchemist image: HTTP ${response.status}`);
      const source = await createImageBitmap(await response.blob());
      try {
        signal.throwIfAborted();
        // Match the existing Canvas2D nearest-neighbour sampler exactly.
        // ImageBitmap's resizeQuality='pixelated' uses a different sampler.
        const surface = new OffscreenCanvas(pixels, pixels);
        try {
          const context = surface.getContext('2d');
          if (!context) throw new Error('Alchemist canvas unavailable');
          context.imageSmoothingEnabled = false;
          context.drawImage(source, 0, 0, pixels, pixels);
          images[key as keyof AlchemistBitmaps] = surface.transferToImageBitmap();
        } finally { surface.width = surface.height = 0; }
      } finally { source.close(); }
      // Decoding itself cannot be aborted; close even a late completion.
      signal.throwIfAborted();
    }
    return images as AlchemistBitmaps;
  } catch (error) {
    closeAlchemistBitmaps(images);
    throw error;
  }
}
