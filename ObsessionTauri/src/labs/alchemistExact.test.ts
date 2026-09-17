import { describe, it, expect, vi } from 'vitest';
import { drawExactAlchemist, exactPoseAt, neutralExactPose, exactFiles, type ExactImages } from './alchemistExact';

describe('source-faithful alchemist', () => {
  it('has an exactly neutral rest pose and honours no-motion mode', () => {
    expect(exactPoseAt(0, 'brew')).toEqual(neutralExactPose);
    for (const t of [750, 1250, 2250, 4050, 11500, 18850]) expect(exactPoseAt(t, 'brew', 'still')).toEqual(neutralExactPose);
  });
  it('uses multiple eased intermediate poses, not sign-selected replacement frames', () => {
    const frames = Array.from({ length: 270 }, (_, i) => exactPoseAt(1400 + i * 10, 'rest').tail);
    expect(new Set(frames).size).toBeGreaterThan(20);
    for (let i = 1; i < frames.length; i++) expect(Math.abs(frames[i] - frames[i - 1])).toBeLessThanOrEqual(.21);
  });
  it('keeps motion bounded, finite and periodic', () => {
    for (let t = 0; t < 23000; t += 17) {
      const pose = exactPoseAt(t, 'brew');
      expect(Math.abs(pose.tail)).toBeLessThanOrEqual(4);
      expect(Math.abs(pose.leftEar)).toBeLessThanOrEqual(2.8);
      expect(Math.abs(pose.rightEar)).toBeLessThanOrEqual(2.8);
      expect(pose.blink).toBeGreaterThanOrEqual(0);
      expect(pose.blink).toBeLessThanOrEqual(1);
      expect(exactPoseAt(t + 23000, 'brew')).toEqual(pose);
    }
  });
  it('isolates ear and tail previews', () => {
    for (let t = 0; t < 23000; t += 31) {
      expect(exactPoseAt(t, 'rest', 'ears').tail).toBe(0);
      expect(exactPoseAt(t, 'rest', 'tail').leftEar).toBe(0);
      expect(exactPoseAt(t, 'rest', 'tail').rightEar).toBe(0);
    }
  });
  it('avoids long frozen poses while retaining quiet low-amplitude motion', () => {
    for (const mood of ['rest', 'brew', 'ready'] as const) {
      let previous = '', unchangedMs = 0, longestStillMs = 0, activeSamples = 0;
      for (let t = 0; t < 23000; t += 25) {
        const pose = exactPoseAt(t, mood);
        const key = JSON.stringify([pose.tail, pose.leftEar, pose.rightEar, pose.blink]);
        unchangedMs = key === previous ? unchangedMs + 25 : 0;
        longestStillMs = Math.max(longestStillMs, unchangedMs);
        if (pose.tail || pose.leftEar || pose.rightEar || pose.blink) activeSamples++;
        previous = key;
      }
      expect(longestStillMs).toBeLessThan(1000);
      expect(activeSamples / (23000 / 25)).toBeGreaterThan(.9);
    }
  });
  it('does not jump at the automatic and tail-only loop boundaries', () => {
    expect(Math.abs(exactPoseAt(22999, 'brew').tail - exactPoseAt(0, 'brew').tail)).toBeLessThanOrEqual(.2);
    expect(Math.abs(exactPoseAt(6199, 'brew', 'tail').tail - exactPoseAt(0, 'brew', 'tail').tail)).toBeLessThanOrEqual(.2);
    expect(exactPoseAt(6199, 'rest', 'ears')).toEqual(exactPoseAt(0, 'rest', 'ears'));
  });
  it.each([104, 240, 440, 880, 1254])('renders the unmodified full source in neutral at %i', pixels => {
    const context = { drawImage: vi.fn(), imageSmoothingEnabled: true };
    const images = Object.fromEntries(Object.keys(exactFiles).map(name => [name, { name }])) as unknown as ExactImages;
    drawExactAlchemist({ getContext: () => context } as unknown as HTMLCanvasElement, images, neutralExactPose, pixels);
    expect(context.drawImage).toHaveBeenCalledOnce();
    expect(context.drawImage).toHaveBeenCalledWith(images.original, 0, 0, pixels, pixels);
    expect(context.imageSmoothingEnabled).toBe(false);
  });
  it('uses fixed-size original cutouts and balanced transforms during motion', () => {
    const context = { drawImage: vi.fn(), imageSmoothingEnabled: true, save: vi.fn(), restore: vi.fn(), translate: vi.fn(), rotate: vi.fn() };
    const images = Object.fromEntries(Object.keys(exactFiles).map(name => [name, { name }])) as unknown as ExactImages;
    drawExactAlchemist({ getContext: () => context } as unknown as HTMLCanvasElement, images, { ...neutralExactPose, tail: 4, leftEar: -2.8 }, 440);
    expect(context.save).toHaveBeenCalledTimes(3);
    expect(context.restore).toHaveBeenCalledTimes(3);
    for (const call of context.drawImage.mock.calls) expect(call.slice(-2)).toEqual([440, 440]);
    expect(context.drawImage.mock.calls[context.drawImage.mock.calls.length - 1]?.[0]).toBe(images.roots);
  });
});
