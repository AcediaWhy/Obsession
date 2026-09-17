import { describe, it, expect } from 'vitest';
import { alchemistTwitchAt, twitchRowShift } from './alchemistTwitch';

describe('local ear and tail twitches', () => {
  it('rests between gestures and loops without a jump', () => {
    expect(alchemistTwitchAt(0)).toEqual({ tail: 0, leftEar: 0, rightEar: 0 });
    expect(alchemistTwitchAt(14000)).toEqual(alchemistTwitchAt(0));
    expect(alchemistTwitchAt(6000)).toEqual(alchemistTwitchAt(0));
    expect(alchemistTwitchAt(13999)).toEqual(alchemistTwitchAt(0));
  });
  it('staggers the ears and keeps tail separate', () => {
    expect(alchemistTwitchAt(3490)).toEqual({ tail: 0, leftEar: -16, rightEar: 0 });
    expect(alchemistTwitchAt(3690).rightEar).toBe(16);
    expect(alchemistTwitchAt(2050).tail).toBe(24);
    expect(alchemistTwitchAt(8190).tail).toBe(-24);
  });
  it('holds each pose instead of interpolating fractional motion', () => {
    expect(alchemistTwitchAt(1800)).toEqual(alchemistTwitchAt(1919));
  });
  it.each([104, 240, 440, 880])('anchors the join and uses only integer offsets at %i', pixels => {
    const height = Math.round(250 * pixels / 1254);
    for (const amplitude of [-24, -16, -8, 8, 16, 24]) {
      expect(twitchRowShift(amplitude, height - 1, height, pixels / 1254)).toBe(0);
      for (let row = 0; row < height; row++) {
        expect(Number.isInteger(twitchRowShift(amplitude, row, height, pixels / 1254))).toBe(true);
      }
    }
  });
});
