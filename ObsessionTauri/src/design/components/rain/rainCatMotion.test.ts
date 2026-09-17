import { describe,it,expect } from 'vitest';
import { rainCatPose } from '../../../labs/rainCatRig.js';
import { RainCatMotion } from './rainCatMotion';

describe('production Rain cat motion',()=>{
  it('keeps the approved idle unchanged',()=>{
    const motion=new RainCatMotion('idle');
    for(let i=0;i<120;i++)expect(motion.step(1/60)).toEqual(rainCatPose(motion.time));
  });
  it('starts a transition from the currently displayed pose',()=>{
    const motion=new RainCatMotion('focused');
    for(let i=0;i<120;i++)motion.step(1/60);
    const before=motion.step(0);
    motion.setPhase('scanning');
    expect(motion.step(0)).toEqual(before);
    const after=motion.step(0.25);
    expect(after).not.toEqual(before);
  });
  it('does not restart the fault on identical signal updates',()=>{
    const motion=new RainCatMotion('fault');
    for(let i=0;i<240;i++){motion.setPhase('fault');motion.step(1/60);}
    expect(motion.step(0).stretch).toBe(-0.012);
  });
  it('keeps paused posters stable with eyes open and time frozen',()=>{
    const motion=new RainCatMotion('focused');
    const first=motion.step(0,true);
    expect(motion.step(0.25,true)).toEqual(first);
    expect(motion.time).toBe(0);
    expect(first.blink).toBe(0);
    expect(first.squint).toBe(0.44);
    motion.setPhase('fault');
    expect(motion.step(0,true).stretch).toBe(-0.012);
  });
  it('clamps a long frame after suspension',()=>{
    const motion=new RainCatMotion('idle');
    motion.step(60);
    expect(motion.time).toBe(0.25);
  });
});
