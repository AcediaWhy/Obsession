import { test } from 'node:test';
import assert from 'node:assert/strict';
import { rainCatPose } from './rainCatRig.js';

const states=['idle','engaging','scanning','focused','fault'];
test('all five states stay finite and within safe deformation bounds',()=>{
  for(const state of states)for(const strength of [0.5,1,1.5])for(let frame=0;frame<2400;frame++){
    const p=rainCatPose(frame/120,{state,strength,windAt:3,blinkAtTime:4});
    assert.ok(Object.values(p).every(Number.isFinite));
    assert.ok(p.blink>=0&&p.blink<=1);
    assert.ok(Math.abs(p.stretch)<0.15);
    assert.ok(Math.abs(p.umbrella)<0.25);
    assert.ok(Math.abs(p.look)<=24);
    assert.ok(p.squint>=0&&p.squint<0.7);
  }
});
test('neutral returns the exact source pose in every state',()=>{
  for(const state of states)for(const time of [0,0.2,1,5,12]){
    assert.ok(Object.values(rainCatPose(time,{state,neutral:true,windAt:0})).every(v=>v===0));
  }
});
test('idle retains its approved breathing, lean, tail and umbrella curves',()=>{
  const tau=Math.PI*2;
  for(let i=0;i<800;i++){
    const t=i/60,p=rainCatPose(t);
    assert.equal(p.breath,(1-Math.cos(t*tau/4.2))*0.5);
    assert.equal(p.lean,Math.sin(t*tau/8)*0.007);
    assert.equal(p.tail,Math.sin(t*tau/4.3)*10);
    assert.equal(p.umbrella,Math.sin(t*tau/6.4)*0.019);
    assert.equal(p.stretch,0);assert.equal(p.squint,0);assert.equal(p.eyeY,0);
  }
});
test('engaging rises and focused quiets down without replacing the artwork',()=>{
  const idle=rainCatPose(2),ready=rainCatPose(2,{state:'engaging'}),focus=rainCatPose(2,{state:'focused'});
  assert.ok(ready.stretch>0.08);
  assert.ok(Math.abs(focus.tail)<Math.abs(idle.tail));
  assert.ok(focus.squint>0.4);
  assert.ok(focus.stretch<0);
});
test('scanning looks in both directions',()=>{
  assert.ok(rainCatPose(1,{state:'scanning'}).look<-8);
  assert.ok(rainCatPose(4,{state:'scanning'}).look>8);
  assert.ok(rainCatPose(1,{state:'scanning'}).lean<-0.06);
  assert.ok(rainCatPose(4,{state:'scanning'}).lean>0.06);
});
test('engaging has a distinct rise and release, not a continuous idle sway',()=>{
  const rise=rainCatPose(1.5,{state:'engaging'}),release=rainCatPose(4,{state:'engaging'});
  assert.ok(rise.stretch-release.stretch>0.06);
  assert.ok((rise.umbrella-rainCatPose(1.5).umbrella)<(release.umbrella-rainCatPose(4).umbrella)-0.05);
});
test('fault startles once, settles, and can be replayed with a fresh stateAt',()=>{
  const startled=rainCatPose(0.3,{state:'fault'}),settled=rainCatPose(4,{state:'fault'});
  assert.ok(startled.stretch<-0.04);
  assert.equal(settled.stretch,-0.012);
  assert.equal(rainCatPose(11,{state:'fault'}).stretch,-0.012);
  assert.ok(rainCatPose(11.3,{state:'fault',stateAt:11}).stretch<-0.04);
});
