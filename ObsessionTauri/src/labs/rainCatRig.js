const TAU = Math.PI * 2;
const smooth = (v) => { const x = Math.max(0, Math.min(1, v)); return x*x*(3-2*x); };
function blinkAt(t) {
  if (t < 0 || t > 0.28) return 0;
  return t < 0.1 ? smooth(t/0.1) : 1-smooth((t-0.1)/0.18);
}

function earTwitch(t) {
  if (t <= 0 || t >= 0.72) return 0;
  return Math.sin(Math.PI*t/0.72)**2 * (1-0.25*Math.sin(t*TAU/0.72));
}

// Coordinates in the 512px layer: tips near y=175, fixed base at y=214.
function earBendOffset(y, tipOffset) {
  return smooth((214-y)/39)*tipOffset*512/1254;
}

export function rainCatPose(time, { strength = 1, neutral = false, windAt = -100, blinkAtTime = -100, state = 'idle', stateAt = 0 } = {}) {
  const a = neutral ? 0 : strength;
  const gustTime = time-windAt;
  const gust = gustTime>0 && gustTime<4 ? Math.sin(Math.PI*gustTime/4)**2 : 0;
  const pose = {
    breath: (1-Math.cos(time*TAU/4.2))*0.5*a,
    lean: Math.sin(time*TAU/8)*0.007*a,
    umbrella: (Math.sin(time*TAU/6.4)*0.019 + gust*0.043*Math.sin(gustTime*2.1))*a,
    tail: (Math.sin(time*TAU/4.3)*10 + gust*Math.sin(time*TAU/1.6)*5)*a,
    blink: neutral ? 0 : Math.max(blinkAt(time-blinkAtTime),blinkAt(time%6.2-2.3),blinkAt(time%6.2-2.73)*0.75),
    look: Math.sin(time*TAU/9)*2*a,
    earLeft: (-8*earTwitch(time%8.8-1.2)-4*earTwitch(time%8.8-5.6)-gust*3)*a,
    earRight: (6*earTwitch(time%8.8-1.43)+gust*2.5)*a,
    stretch: 0,
    squint: 0,
    eyeY: 0,
  };
  const elapsed = Math.max(0,time-stateAt);
  const enter = smooth(elapsed/0.65)*a;
  if (state === 'engaging') {
    // Anticipation, rise, hold, release: an action phrase rather than another sway.
    const beat = elapsed%4.6;
    const prepare = smooth(beat/0.25)*(1-smooth((beat-0.3)/0.35));
    const ready = smooth((beat-0.35)/0.45)*(1-smooth((beat-2.4)/1.1));
    pose.stretch = (0.025-prepare*0.05+ready*0.07)*enter;
    pose.lean += (-0.018-ready*0.025)*enter;
    pose.umbrella += (-0.025-ready*0.06)*enter;
    pose.tail += (Math.sin(beat*TAU/1.5)*16*(1-ready*0.6)-ready*8)*enter;
    pose.earLeft += (12+ready*9)*enter;
    pose.earRight -= (12+ready*9)*enter;
    pose.eyeY = (-3-ready*6)*enter;
  } else if (state === 'scanning') {
    // Ease between held gazes, giving each direction time to read.
    const phase = (elapsed%8)/8;
    const gaze = phase<0.5 ? -1+2*smooth((phase-0.12)/0.24) : 1-2*smooth((phase-0.62)/0.24);
    pose.look += gaze*14*enter;
    pose.lean += gaze*0.075*enter;
    // Counter-tilt the canopy slightly while the cat peeks out to either side.
    pose.umbrella -= gaze*0.045*enter;
    pose.stretch = -0.025*Math.abs(gaze)*enter;
    pose.earLeft += (-14*earTwitch(elapsed%8-0.5)+13*gaze)*enter;
    pose.earRight += (14*earTwitch(elapsed%8-4.5)+13*gaze)*enter;
    pose.tail *= 0.65;
    pose.eyeY = -4*enter;
  } else if (state === 'focused') {
    const settle = smooth(elapsed/0.65);
    pose.lean *= 1-settle*0.8;
    pose.tail *= 1-settle*0.78;
    pose.umbrella *= 1-settle*0.7;
    pose.look *= 1-settle*0.85;
    pose.earLeft *= 1-settle*0.8;
    pose.earRight *= 1-settle*0.8;
    pose.stretch = -0.035*enter;
    pose.lean += -0.022*enter;
    pose.umbrella -= 0.065*enter;
    pose.earLeft += 13*enter;
    pose.earRight -= 13*enter;
    pose.squint = 0.44*enter;
    pose.eyeY = 4*enter;
  } else if (state === 'fault') {
    // One startle on entry, not an endless shake. Tail and ear roots stay pinned.
    const startle = smooth(elapsed/0.14)*(1-smooth((elapsed-0.28)/1.15))*a;
    const tremble = Math.sin(elapsed*TAU*2.5)*startle;
    pose.stretch = -0.045*startle-0.012*enter;
    pose.lean += 0.014*enter+0.009*tremble;
    pose.umbrella += 0.075*startle+0.02*enter;
    pose.earLeft -= 22*startle+7*enter;
    pose.earRight += 22*startle+7*enter;
    pose.tail += -15*startle+3*tremble;
    pose.eyeY = -4*startle;
    pose.squint = 0.08*enter;
  }
  return pose;
}

export function createRainCatRig(canvas, images) {
  const ctx = canvas.getContext('2d');
  const tailCanvas = document.createElement('canvas');
  tailCanvas.width = tailCanvas.height = 512;
  const tailCtx = tailCanvas.getContext('2d');
  const bodyCanvas = document.createElement('canvas');
  bodyCanvas.width = bodyCanvas.height = 512;
  const bodyCtx = bodyCanvas.getContext('2d');
  let disposed = false;
  function bodyTransform(pose) {
    ctx.translate(632,933); ctx.rotate(pose.lean);
    ctx.scale(1+pose.breath*0.008-pose.stretch*0.25,1+pose.breath*0.018+pose.stretch); ctx.translate(-632,-933);
  }
  function drawLayer(name) { ctx.drawImage(images[name],0,0,1254,1254); }
  function umbrellaTransform(pose) { ctx.translate(514,752); ctx.rotate(pose.umbrella); ctx.translate(-514,-752); }
  function render(time, options = {}) {
    if (disposed) return;
    const pose = options.pose ?? rainCatPose(time, options);
    ctx.setTransform(canvas.width/1254,0,0,canvas.height/1254,0,0);
    ctx.clearRect(0,0,1254,1254);
    if (options.puddle !== false && images.puddle) drawLayer('puddle');
    ctx.save(); bodyTransform(pose);
    ctx.save(); umbrellaTransform(pose); drawLayer('umbrella'); ctx.restore();
    // Pin the lower curve to the body. Warp only rows above the attachment.
    tailCtx.clearRect(0,0,512,512);
    tailCtx.drawImage(images.tail,0,355,512,157,0,355,512,157);
    for (let y=0;y<355;y++) {
      const free=smooth((355-y)/58);
      tailCtx.drawImage(images.tail,0,y,512,1,pose.tail*free*512/1254,y,512,1);
    }
    ctx.drawImage(tailCanvas,0,0,1254,1254);
    if (pose.earLeft === 0 && pose.earRight === 0) drawLayer('body');
    else {
      bodyCtx.clearRect(0,0,512,512);
      bodyCtx.drawImage(images.body,0,214,512,298,0,214,512,298);
      // Above the forehead the ears are separated by transparent space. Each
      // bends independently; the displacement reaches zero at the fixed base.
      for (let y=0;y<214;y++) {
        bodyCtx.drawImage(images.body,0,y,256,1,earBendOffset(y,pose.earLeft),y,256,1);
        bodyCtx.drawImage(images.body,256,y,256,1,256+earBendOffset(y,pose.earRight),y,256,1);
      }
      ctx.drawImage(bodyCanvas,0,0,1254,1254);
    }
    ctx.save(); umbrellaTransform(pose); drawLayer('paw'); ctx.restore();
    ctx.save(); ctx.translate(632+pose.look,646+pose.eyeY); ctx.scale(1,(1-pose.blink*0.965)*(1-pose.squint)); ctx.translate(-632,-646); drawLayer('eyes'); ctx.restore();
    ctx.restore();
    if (options.rain && !options.neutral) {
      // Drops originate at canopy edges, never travel through the cat's face.
      ctx.save(); bodyTransform(pose); umbrellaTransform(pose);
      ctx.strokeStyle='#dbe5e8';ctx.lineWidth=2;ctx.lineCap='round';
      for (let i=0;i<4;i++) {
        const f=(time*0.66+i*0.27)%1;
        const x=i%2 ? 952 : 215, y=i%2 ? 579 : 543;
        ctx.globalAlpha=Math.sin(Math.PI*f)*0.55;
        ctx.beginPath();ctx.moveTo(x,y+f*f*310);ctx.lineTo(x,y+f*f*310+7);ctx.stroke();
      }
      ctx.restore();
      ctx.strokeStyle='#d0d9de';ctx.lineWidth=1.8;
      for (let i=0;options.puddle !== false && i<3;i++) {
        const f=(time*0.6+i/3)%1;
        ctx.globalAlpha=(1-f)*0.4;
        ctx.beginPath();ctx.ellipse([396,885,776][i],[952,966,1017][i],4+f*28,1+f*5,0,0,TAU);ctx.stroke();
      }
      ctx.globalAlpha=1;
    }
    canvas.dataset.pose=JSON.stringify(pose);
  }
  return {render,dispose(){disposed=true;tailCanvas.width=tailCanvas.height=0;bodyCanvas.width=bodyCanvas.height=0;ctx.clearRect(0,0,1254,1254);}};
}
