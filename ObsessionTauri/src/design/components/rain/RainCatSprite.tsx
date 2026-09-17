import { useEffect, useRef, useState } from 'react';
import { createRainCatRig, type RainCatImages, type RainCatRig } from '../../../labs/rainCatRig.js';
import type { ObsessionVisualPhase } from '../../obsessionVisualState';
import type { RenderLoop } from '../../render';
import { RainCatMotion } from './rainCatMotion';
import './rainCatSprite.css';

const source = `${import.meta.env.BASE_URL}lab-assets/rain-cat-user-v2/`;
const layers = ['umbrella','tail','body','paw','eyes'] as const;
let imagesPromise: Promise<RainCatImages> | undefined;
function loadLayers() {
  if (!imagesPromise) imagesPromise = Promise.all(layers.map(async name => {
    const image = new Image();
    image.src = `${source}${name}.webp`;
    await image.decode();
    return [name,image] as const;
  })).then(entries => Object.fromEntries(entries) as RainCatImages).catch(error => {
    imagesPromise = undefined;
    throw error;
  });
  return imagesPromise;
}

export function RainCatSprite({ phase, paused = false, size = 240 }: {
  phase: ObsessionVisualPhase; paused?: boolean; size?: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop>();
  const motionRef = useRef<RainCatMotion>();
  const propsRef = useRef({ phase, paused });
  propsRef.current = { phase, paused };
  const [ready,setReady] = useState(false);
  const role = size >= 160 ? 'hero' : 'preview';

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let disposed = false;
    let rig: RainCatRig | undefined;
    let loop: RenderLoop | undefined;
    setReady(false);
    // Small theme previews are optically enlarged in CSS; render enough pixels
    // for their displayed size instead of stretching a 104px backing canvas.
    canvas.width = canvas.height = Math.min(768, Math.ceil(size*Math.min(window.devicePixelRatio || 1,2)*(role === 'preview' ? 1.65 : 1)));
    void Promise.all([loadLayers(),import('../../render')]).then(([images,{ createRenderLoop }]) => {
      if (disposed) return;
      rig = createRainCatRig(canvas,images);
      const motion = new RainCatMotion(propsRef.current.phase);
      motionRef.current = motion;
      const draw = (dt: number) => {
        const { phase: nextPhase, paused: stopped } = propsRef.current;
        motion.setPhase(nextPhase);
        const pose = motion.step(stopped ? 0 : dt,stopped);
        rig?.render(motion.time,{ pose, rain: !stopped, puddle: false });
      };
      draw(0);
      loop = createRenderLoop(draw,{ role, fps: role === 'hero' ? 60 : 30, paused: propsRef.current.paused });
      loopRef.current = loop;
      loop.start();
      setReady(true);
    }).catch(() => {
      loop?.dispose();
      rig?.dispose();
      if (!disposed) setReady(false);
    });
    return () => {
      disposed = true;
      loop?.dispose();
      rig?.dispose();
      loopRef.current = undefined;
      motionRef.current = undefined;
      canvas.width = canvas.height = 0;
    };
  },[size,role]);

  useEffect(() => {
    motionRef.current?.setPhase(phase);
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  },[phase,paused]);

  return <span className="rain-cat-sprite" aria-hidden="true" data-rain-cat-sprite
    data-phase={phase} data-motion={paused ? 'still' : 'running'} data-rig-ready={ready} data-puddle="false"
    style={{ width: size, height: size }}>
    <span className="rain-cat-sprite__still">
      {layers.map(layer => <img key={layer} src={`${source}${layer}.webp`} alt="" draggable={false} />)}
    </span>
    <canvas ref={canvasRef} className="rain-cat-sprite__canvas" />
  </span>;
}
