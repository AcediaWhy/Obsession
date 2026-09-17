import type { EyeFrame } from './alchemistLayers';

export type AlchemistMood = 'rest' | 'brew' | 'ready';
export type AlchemistAction = 'idle' | 'inspect' | 'lift' | 'bubble' | 'settle';
export interface AlchemistPose { eye: EyeFrame; flaskY: number; action: AlchemistAction }

// All times are milliseconds. Motion is intentionally held on discrete poses;
// raster displacement is rounded again to actual device pixels when rendered.
export function alchemistPoseAt(time: number, mood: AlchemistMood): AlchemistPose {
  const t = ((time % 14000) + 14000) % 14000;
  const blink = (at: number): EyeFrame => at < 60 ? 'half' : at < 180 ? 'closed' : at < 240 ? 'half' : 'open';
  const idle: AlchemistPose = { eye: t >= 10500 && t < 10740 ? blink(t - 10500) : 'open', flaskY: 0, action: 'idle' };
  if (mood !== 'brew') return idle;
  if (t < 3200 || t >= 6800) return idle;
  if (t < 4000) return { eye: 'half', flaskY: 0, action: 'inspect' };
  if (t < 4600) return { eye: 'half', flaskY: -6 * (1 + Math.floor((t - 4000) / 150)), action: 'lift' };
  if (t < 5800) return { eye: 'open', flaskY: -24, action: 'bubble' };
  if (t < 6040) return { eye: blink(t - 5800), flaskY: -24, action: 'bubble' };
  if (t < 6200) return { eye: 'open', flaskY: -24, action: 'bubble' };
  return { eye: 'open', flaskY: -24 + 6 * (1 + Math.floor((t - 6200) / 150)), action: 'settle' };
}
