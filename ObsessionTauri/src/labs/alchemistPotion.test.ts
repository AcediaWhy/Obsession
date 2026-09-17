import { describe, it, expect, vi } from 'vitest';
import { potionFrameAt, drawPotionFrame } from './alchemistPotion';

describe('alchemist potion overlay', () => {
  it('leaves the reference and reduced-motion view untouched', () => {
    expect(potionFrameAt(5000, 'brew', false)).toEqual({ bubbles: [], glow: 0, glint: -1, puff: -1 });
  });
  it('bubbles rise to the liquid surface and pop', () => {
    expect(potionFrameAt(1600, 'brew').bubbles[0].y).toBeLessThan(potionFrameAt(350, 'brew').bubbles[0].y);
    expect(potionFrameAt(2450, 'brew').bubbles[0].pop).toBe(true);
  });
  it('uses quiet states outside brewing', () => {
    for (const mood of ['rest', 'ready'] as const) {
      expect(potionFrameAt(5100, mood).bubbles).toEqual([]);
      expect(potionFrameAt(5100, mood).puff).toBe(-1);
    }
    expect(potionFrameAt(2500, 'rest').glint).toBe(-1);
  });
  it('brightens before a short mouth puff and keeps glints occasional', () => {
    expect(potionFrameAt(4550, 'brew').glow).toBeGreaterThan(potionFrameAt(0, 'brew').glow);
    expect(potionFrameAt(4550, 'brew').puff).toBe(-1);
    expect(potionFrameAt(5100, 'brew').puff).toBeGreaterThan(0);
    expect(potionFrameAt(6000, 'brew').puff).toBe(-1);
    expect(potionFrameAt(2600, 'brew').glint).toBeCloseTo(.4);
    expect(potionFrameAt(4000, 'brew').glint).toBe(-1);
  });
  it('is deterministic for pause, replay and scrubbing, and bounds every particle', () => {
    for (let t = 0; t < 23000; t += 61) {
      const frame = potionFrameAt(t, 'brew');
      expect(potionFrameAt(t + 23000, 'brew')).toEqual(frame);
      for (const b of frame.bubbles) {
        expect(b.alpha).toBeGreaterThanOrEqual(0);
        expect(b.alpha).toBeLessThanOrEqual(.72);
        expect(b.y).toBeGreaterThanOrEqual(896);
        expect(b.y).toBeLessThanOrEqual(1050);
      }
    }
  });
  it.each([104, 240, 440])('clears and clips the pixel overlay at %i px', pixels => {
    const ctx = { clearRect: vi.fn(), save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), closePath: vi.fn(), clip: vi.fn(), fillRect: vi.fn(), imageSmoothingEnabled: true };
    const canvas = { width: pixels, height: pixels, getContext: () => ctx } as unknown as HTMLCanvasElement;
    drawPotionFrame(canvas, potionFrameAt(5100, 'brew'), pixels);
    expect(ctx.clearRect).toHaveBeenCalledWith(0, 0, pixels, pixels);
    expect(ctx.imageSmoothingEnabled).toBe(false);
    expect(ctx.clip).toHaveBeenCalled();
    expect(ctx.save.mock.calls.length).toBe(ctx.restore.mock.calls.length);
    for (const args of ctx.fillRect.mock.calls) for (const n of args) expect(Number.isInteger(n)).toBe(true);
  });
});
