import { describe, it, expect, vi } from 'vitest';
import { alchemistPoseAt } from './alchemistMotion';
import { drawAlchemistFrame, type LayerImages } from './alchemistLayers';

describe('alchemist little ritual', () => {
  it('looks, raises, reacts, blinks, lowers, and rests', () => {
    expect(alchemistPoseAt(0, 'brew').action).toBe('idle');
    expect(alchemistPoseAt(3500, 'brew')).toEqual({ eye: 'half', flaskY: 0, action: 'inspect' });
    expect(alchemistPoseAt(4150, 'brew').flaskY).toBe(-12);
    expect(alchemistPoseAt(4700, 'brew')).toEqual({ eye: 'open', flaskY: -24, action: 'bubble' });
    expect(alchemistPoseAt(5900, 'brew').eye).toBe('closed');
    expect(alchemistPoseAt(6350, 'brew').flaskY).toBe(-12);
    expect(alchemistPoseAt(6800, 'brew')).toEqual({ eye: 'open', flaskY: 0, action: 'idle' });
    expect(alchemistPoseAt(17500, 'brew')).toEqual(alchemistPoseAt(3500, 'brew'));
  });
  it('does not brew in rest or ready modes', () => {
    for (const mood of ['rest', 'ready'] as const) {
      for (let time = 0; time < 14000; time += 50) {
        expect(alchemistPoseAt(time, mood).flaskY).toBe(0);
        expect(alchemistPoseAt(time, mood).action).toBe('idle');
      }
      expect(alchemistPoseAt(10600, mood).eye).toBe('closed');
    }
  });
  it('holds each lift step and never overshoots', () => {
    for (let time = 0; time < 14000; time += 10) {
      expect([0, -6, -12, -18, -24]).toContain(alchemistPoseAt(time, 'brew').flaskY);
    }
    expect(alchemistPoseAt(4000, 'brew')).toEqual(alchemistPoseAt(4149, 'brew'));
  });
  it.each([104, 208, 240, 312, 440, 880])('keeps the flask dimensions and body unchanged while lifting at %i px', pixels => {
    const images = { body: { naturalWidth: 1241, naturalHeight: 1267 }, flask: {}, open: {}, half: {}, closed: {} } as LayerImages;
    const poses = [0, -6, -12, -18, -24].map(y => {
      const context = { imageSmoothingEnabled: true, drawImage: vi.fn() };
      drawAlchemistFrame({ getContext: () => context } as unknown as HTMLCanvasElement, images, 'open', pixels, y);
      return context.drawImage.mock.calls;
    });
    for (const calls of poses) {
      expect(calls[0]).toEqual(poses[0][0]);
      expect(calls.slice(2)).toEqual(poses[0].slice(2));
      expect(calls[1].slice(7)).toEqual(poses[0][1].slice(7));
      expect(calls[1].slice(5).every(Number.isInteger)).toBe(true);
    }
    expect(poses[4][1][6]).toBeLessThan(poses[0][1][6]);
  });
});
