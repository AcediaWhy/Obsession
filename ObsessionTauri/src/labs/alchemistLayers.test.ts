import { describe, it, expect, vi } from 'vitest';
import { drawAlchemistFrame, eyeFrames, eyePlacement, type LayerImages } from './alchemistLayers';

describe('cleaned alchemist layers', () => {
  const images = { body: { naturalWidth: 1241, naturalHeight: 1267 }, flask: {}, open: {}, half: {}, closed: {} } as LayerImages;
  it.each([104, 208, 240, 312, 440, 880])('snaps every target edge to physical pixels at %i', pixels => {
    const context = { imageSmoothingEnabled: true, drawImage: vi.fn() };
    const canvas = { width: 0, height: 0, getContext: () => context } as unknown as HTMLCanvasElement;
    for (const frame of eyeFrames) {
      context.drawImage.mockClear();
      drawAlchemistFrame(canvas, images, frame, pixels);
      expect(context.imageSmoothingEnabled).toBe(false);
      expect(context.drawImage).toHaveBeenCalledTimes(4);
      for (const call of context.drawImage.mock.calls) {
        expect(call.slice(5).every(Number.isInteger)).toBe(true);
        expect(call[7]).toBeGreaterThan(0);
        expect(call[8]).toBeGreaterThan(0);
      }
    }
  });
  it('does not shift the body or flask when the eyes blink', () => {
    const calls = eyeFrames.map(frame => {
      const context = { drawImage: vi.fn(), imageSmoothingEnabled: true };
      drawAlchemistFrame({ getContext: () => context } as unknown as HTMLCanvasElement, images, frame, 440);
      return context.drawImage.mock.calls.slice(0, 2);
    });
    expect(calls[1]).toEqual(calls[0]);
    expect(calls[2]).toEqual(calls[0]);
  });
  it('keeps the eye width and lower eyelid anchors across frames', () => {
    for (const side of [0, 1]) {
      const anchors = eyeFrames.map(frame => {
        const [x, y, width, height] = eyePlacement[frame].target[side];
        return [x, width, y + height];
      });
      expect(anchors[1]).toEqual(anchors[0]);
      expect(anchors[2]).toEqual(anchors[0]);
    }
  });
});
