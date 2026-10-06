import { useEffect, useRef } from "react";
import { useMotionOff, useRenderHidden } from "../render";
import { createDotVeil } from "./patternWaves/dotVeil";

export function Zapret2CardBackground({category,interactive}:{category:string;interactive:boolean}) {
  const ref=useRef<HTMLDivElement>(null);
  const hidden=useRenderHidden();
  const motionOff=useMotionOff();
  const scene=useRef<ReturnType<typeof createDotVeil> | null>(null);
  const latest=useRef({interactive,motionOff,hidden});
  latest.current={interactive,motionOff,hidden};
  useEffect(()=>{
    if(!ref.current)return;
    scene.current=createDotVeil(ref.current,latest.current);
    return ()=>{scene.current?.dispose();scene.current=null;};
  },[]);
  useEffect(()=>{scene.current?.update({interactive,motionOff,hidden});},[interactive,motionOff,hidden]);
  return <div className="z2-card-background" data-service={category} aria-hidden="true">
    <div ref={ref} className="z2-card-dotfield" />
    <div className="z2-card-grain" />
  </div>;
}
