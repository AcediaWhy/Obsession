import { describe, expect, it, vi } from 'vitest';
import { drawAlchemistFrame, separatedParts, separatedPose, type LayerImages } from './alchemistLayers';

const images = Object.fromEntries(['body', 'hat', 'tail', 'leftEar', 'rightEar', 'flask', 'open', 'half', 'closed'].map(name => [name, { name, naturalWidth: 1254, naturalHeight: 1254 }])) as unknown as LayerImages;
const render = (pixels: number, amplitude: number, eye: 'open' | 'closed' = 'open') => {
  const context = { drawImage: vi.fn(), save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), closePath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), clip: vi.fn(), imageSmoothingEnabled: true };
  drawAlchemistFrame({ getContext: () => context } as unknown as HTMLCanvasElement, images, eye, pixels, 0, { tail: amplitude, leftEar: amplitude, rightEar: amplitude });
  return context;
};

describe('separated alchemist assembly', () => {
  it.each([104, 208, 240, 440, 880])('draws snapped independent layers at %i pixels without row warping', pixels => {
    for (const amplitude of [-24, 0, 24]) {
      const context = render(pixels, amplitude);
      expect(context.imageSmoothingEnabled).toBe(false);
      expect(context.drawImage).toHaveBeenCalledTimes(9);
      expect(context.drawImage.mock.calls.map(call => call[0])).toEqual([images.tail, images.body, images.hat, images.leftEar, images.rightEar, images.hat, images.flask, images.open, images.open]);
      for (const call of context.drawImage.mock.calls) expect(call.slice(5).every(Number.isInteger)).toBe(true);
      expect(context.save).toHaveBeenCalledOnce();
      expect(context.restore).toHaveBeenCalledOnce();
      expect(context.clip).toHaveBeenCalledOnce();
    }
  });
  it('keeps body, hat, flask and eyes fixed during a twitch', () => {
    const still = render(440, 0).drawImage.mock.calls;
    for (const amplitude of [-24, 24]) {
      const moving = render(440, amplitude).drawImage.mock.calls;
      for (const index of [1, 2, 5, 6, 7, 8]) expect(moving[index]).toEqual(still[index]);
    }
  });
  it('only changes eyes when blinking', () => {
    expect(render(440, 0, 'closed').drawImage.mock.calls.slice(0, 7)).toEqual(render(440, 0).drawImage.mock.calls.slice(0, 7));
  });
  it('excludes the inconsistent right-ear third pose and keeps crops within sheets', () => {
    for (const part of ['tail', 'leftEar', 'rightEar'] as const) {
      expect(separatedPose(part, 0)).toBe(0);
      for (const amplitude of [-24, 8, 24]) expect(separatedPose(part, amplitude)).toBeLessThan(separatedParts[part].sources.length);
      for (const [x, y, width, height] of separatedParts[part].sources) {
        expect(x).toBeGreaterThanOrEqual(0);
        expect(y).toBeGreaterThanOrEqual(0);
        expect(x + width).toBeLessThanOrEqual(1672);
        expect(y + height).toBeLessThanOrEqual(941);
      }
    }
  });
});
