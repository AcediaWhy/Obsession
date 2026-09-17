import { useEffect, useRef, useState } from "react";
import type { ObsessionVisualPhase } from "../obsessionVisualState";
import type { PlaneIndex } from "./pixelart/pixelCore";
import { drawSourceArt, loadSourceArt, SOURCE_ART_HEIGHT, SOURCE_ART_WIDTH, POND_FRAME_MS } from "./axolotl/pondGifScene";
import "./SunkenStarFieldLab.css";

export type FieldMetrics = { scale: number; width: number; height: number };
type Props = {
  phase?: ObsessionVisualPhase;
  paused?: boolean;
  isolate?: PlaneIndex | null;
  dither?: boolean;
  scale?: number | null;
  onMetrics?: (metrics: FieldMetrics) => void;
};

export function SunkenStarFieldLab({ phase = "idle", paused = false, onMetrics }: Props) {
  const fieldRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const metricsCallback = useRef(onMetrics);
  metricsCallback.current = onMetrics;
  const [status, setStatus] = useState("loading");
  const [displayScale, setDisplayScale] = useState(1);

  useEffect(() => {
    const field=fieldRef.current, canvas=canvasRef.current;
    if(!field || !canvas) return;
    const context=canvas.getContext("2d",{alpha:false});
    if(!context) { setStatus("error"); return; }
    let disposed=false, animation=0, last=-1;
    const reduce=window.matchMedia("(prefers-reduced-motion: reduce)");
    let stopped=paused || reduce.matches;
    let redraw=()=>{};
    const changed=()=>{ stopped=paused || reduce.matches; last=-1; redraw(); };
    reduce.addEventListener("change",changed);

    const resize=()=>{
      const rect=field.getBoundingClientRect(), ratio=window.devicePixelRatio || 1;
      const width=Math.max(1,Math.round(rect.width*ratio)), height=Math.max(1,Math.round(rect.height*ratio));
      if(canvas.width!==width || canvas.height!==height) {
        canvas.width=width; canvas.height=height;
        canvas.style.width=rect.width+"px"; canvas.style.height=rect.height+"px";
        setDisplayScale(Math.min(width/SOURCE_ART_WIDTH,height/SOURCE_ART_HEIGHT));
        // The overview also uses this legacy scale for its separate hero sprite.
        // Keep that layout scale independent of the source image's display ratio.
        const uiScale=Math.max(2,Math.min(6,Math.round(Math.min(width/450,height/336))));
        metricsCallback.current?.({width:SOURCE_ART_WIDTH,height:SOURCE_ART_HEIGHT,scale:uiScale});
        last=-1;
      }
    };
    const observer=new ResizeObserver(()=>{resize();redraw();});
    observer.observe(field);
    loadSourceArt().then(scene=>{
      if(disposed) return;
      setStatus("ready");
      const start=performance.now();
      const tick=(now:number)=>{
        if(disposed) return;
        resize();
        const frame=stopped?0:Math.floor(Math.max(0,now-start)/POND_FRAME_MS);
        if(frame!==last) {
          drawSourceArt(context,scene,canvas.width,canvas.height,frame*POND_FRAME_MS/1000,stopped);
          last=frame;
        }
        if(!stopped) animation=requestAnimationFrame(tick);
      };
      redraw=()=>{cancelAnimationFrame(animation);tick(performance.now());};
      redraw();
    }).catch(error=>{if(!disposed) {setStatus("error"); console.error(error);}});
    return ()=>{disposed=true;cancelAnimationFrame(animation);observer.disconnect();reduce.removeEventListener("change",changed);};
  },[paused]);

  return <div aria-hidden="true" className="sunken-star-field-lab" data-sunken-star-field-lab
    data-renderer="source-art-canvas" data-phase={phase} data-motion={paused?"paused":"running"}
    data-art-size={`${SOURCE_ART_WIDTH}x${SOURCE_ART_HEIGHT}`} data-pixel-scale={displayScale} data-status={status} ref={fieldRef}>
    <canvas className="sunken-star-field-lab__canvas" ref={canvasRef}/>
  </div>;
}
