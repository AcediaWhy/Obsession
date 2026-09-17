/** Original art stays at native resolution; only small local patches animate. */
export const SOURCE_ART_URL = new URL(
  "../../../../lab-source-assets/makeit-source.jpg",
  import.meta.url,
).href;
export const SOURCE_ART_WIDTH = 2400;
export const SOURCE_ART_HEIGHT = 1792;

type PatchSpec = { x: number; y: number; w: number; h: number; kind: 'sign' | 'leaf'; strength: number; phase: number };
type Patch = PatchSpec & { frames: HTMLCanvasElement[] };
export type SourceArtScene = { image: HTMLImageElement; patches: Patch[]; blink: HTMLCanvasElement[] };

const canvasOf = (w: number, h: number) => {
  const canvas = document.createElement('canvas');
  canvas.width = w; canvas.height = h;
  return canvas;
};

function patchFrames(image: HTMLImageElement, spec: PatchSpec): Patch {
  const { x, y, w, h, kind, strength } = spec;
  const original = canvasOf(w, h);
  const context = original.getContext('2d')!;
  context.drawImage(image, x, y, w, h, 0, 0, w, h);
  const pixels = context.getImageData(0, 0, w, h);
  const source = new Uint32Array(pixels.data.buffer);
  const frames: HTMLCanvasElement[] = [];
  const smooth = (t: number) => { const s = Math.max(0, Math.min(1, t)); return s*s*(3-2*s); };
  // Fixed boundary, rigid sign interior. Surrounding sky absorbs the tiny motion.
  const displacement = new Float32Array(w*h*2);
  for (let row=0; row<h; row++) for (let col=0; col<w; col++) {
    const index = (row*w+col)*2;
    const edge = smooth(Math.min(col,w-1-col)/24) * smooth(Math.min(row,h-1-row)/26);
    if (kind === 'sign') {
      displacement[index] = -(row+12)*0.005*edge;
      displacement[index+1] = (col-w*.5)*0.005*edge;
    } else {
      const tip = smooth(row/(h*.65));
      displacement[index] = strength*edge*tip;
      displacement[index+1] = strength*.2*edge*tip;
    }
  }
  for (let frame=0; frame<9; frame++) {
    if(frame===4) { frames.push(original); continue; }
    const target = canvasOf(w,h), ctx = target.getContext('2d')!;
    const result = ctx.createImageData(w,h), dest = new Uint32Array(result.data.buffer);
    const amount = (frame-4)/4;
    for(let row=0;row<h;row++) for(let col=0;col<w;col++) {
      const i=row*w+col;
      const sx=Math.max(0,Math.min(w-1,col-Math.round(displacement[i*2]*amount)));
      const sy=Math.max(0,Math.min(h-1,row-Math.round(displacement[i*2+1]*amount)));
      dest[i]=source[sy*w+sx];
    }
    ctx.putImageData(result,0,0);
    frames.push(target);
  }
  return {...spec, frames};
}

function blinkFrames(image: HTMLImageElement): HTMLCanvasElement[] {
  const x=1198,y=816,w=124,h=69;
  const original=canvasOf(w,h), ctx=original.getContext('2d')!;
  ctx.drawImage(image,x,y,w,h,0,0,w,h);
  const pixels=ctx.getImageData(0,0,w,h);
  // Include JPEG edge pixels around the pale iris, so a closed eye has no halo.
  const mask=new Uint8Array(w*h);
  for(let row=0;row<h;row++) for(let col=0;col<w;col++) {
    const i=(row*w+col)*4;
    if(pixels.data[i]>115 && pixels.data[i+1]>100 && pixels.data[i+2]>65) {
      for(let dy=-3;dy<=3;dy++) for(let dx=-3;dx<=3;dx++) {
        if(row+dy>=0 && row+dy<h && col+dx>=0 && col+dx<w) mask[(row+dy)*w+col+dx]=1;
      }
    }
  }
  const frames: HTMLCanvasElement[]=[];
  for(const closure of [0,.45,.85,1]) {
    const canvas=canvasOf(w,h), context=canvas.getContext('2d')!;
    const copy=new ImageData(new Uint8ClampedArray(pixels.data),w,h);
    for(let row=0;row<h;row++) for(let col=0;col<w;col++) {
      const i=(row*w+col)*4;
      const eye=col<73 ? {cy:37,ry:28} : {cy:38,ry:25};
      if(closure>0 && mask[row*w+col] && Math.abs(row-eye.cy)>=eye.ry*(1-closure)) {
        copy.data[i]=34; copy.data[i+1]=24; copy.data[i+2]=35;
      }
    }
    context.putImageData(copy,0,0);
    if(closure===1) {
      context.fillStyle='#493443';
      context.fillRect(12,38,40,3); context.fillRect(91,39,24,3);
    }
    frames.push(canvas);
  }
  return frames;
}

let pending: Promise<SourceArtScene> | undefined;
export function loadSourceArt(): Promise<SourceArtScene> {
  if(pending) return pending;
  pending=new Promise((resolve,reject)=>{
    const image=new Image();
    image.onload=()=>{
      try {
        const specs: PatchSpec[] = [
          {x:1468,y:278,w:340,h:650,kind:'sign',strength:1,phase:0},
          {x:1150,y:440,w:145,h:272,kind:'leaf',strength:3,phase:1.4},
          {x:1320,y:649,w:149,h:221,kind:'leaf',strength:3,phase:3.1},
          {x:727,y:1290,w:148,h:374,kind:'leaf',strength:4,phase:4.3},
        ];
        resolve({image,patches:specs.map(spec=>patchFrames(image,spec)),blink:blinkFrames(image)});
      } catch(error) { pending=undefined; reject(error); }
    };
    image.onerror=()=>{pending=undefined;reject(new Error('Не удалось загрузить исходный арт'));};
    image.src=SOURCE_ART_URL;
  });
  return pending;
}

export function drawSourceArt(context: CanvasRenderingContext2D, scene: SourceArtScene, width: number, height: number, seconds: number, still=false) {
  const fit=Math.max(width/SOURCE_ART_WIDTH,height/SOURCE_ART_HEIGHT);
  const left=Math.round((width-SOURCE_ART_WIDTH*fit)/2);
  const top=Math.round((height-SOURCE_ART_HEIGHT*fit)*.43);
  context.imageSmoothingEnabled=false;
  context.setTransform(fit,0,0,fit,left,top);
  context.drawImage(scene.image,0,0);
  if(!still) {
    for(const patch of scene.patches) {
      const frame=Math.round(4+4*Math.sin(seconds*(patch.kind==='sign'?.85:1.1)+patch.phase));
      context.drawImage(patch.frames[frame],patch.x,patch.y);
    }
    // 260 ms blink; two different pauses make the loop less mechanical.
    const clock=seconds%12.9;
    const t=clock>=8.6 ? clock-8.6 : clock-4.1;
    if(t>=0 && t<.28) {
      const index=t<.06?1:t<.1?2:t<.18?3:t<.23?2:1;
      context.drawImage(scene.blink[index],1198,816);
    }
  }
  context.setTransform(1,0,0,1,0,0);
}
