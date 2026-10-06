import { useEffect, useRef, useState } from "react";
import type { PatternOptions, PatternScene } from "./scene";
import { observePatternPalette, type PatternPalette } from "./theme";

export function PatternWavesSurface({ options, className="" }: { options:PatternOptions; className?:string }) {
  const containerRef=useRef<HTMLDivElement>(null);
  const sceneRef=useRef<PatternScene | null>(null);
  const paletteRef=useRef<PatternPalette | null>(null);
  const latest=useRef(options);
  latest.current=options;
  const [ready,setReady]=useState(false);
  useEffect(()=>{
    const container=containerRef.current;
    if(!container)return;
    let disposed=false;
    const stopObserving=observePatternPalette(container,palette=>{
      paletteRef.current=palette;
      sceneRef.current?.update(palette);
    });
    void import("./scene").then(({createPatternWaves})=>{
      if(disposed)return;
      try {
        sceneRef.current=createPatternWaves(container,{...latest.current,...paletteRef.current,onUnavailable:()=>{if(!disposed)setReady(false);}});
        setReady(true);
      } catch {
        container.querySelectorAll("canvas").forEach(canvas=>{
          canvas.getContext("webgl2")?.getExtension("WEBGL_lose_context")?.loseContext();
          canvas.remove();
        });
        setReady(false);
      }
    }).catch(()=>{});
    return ()=>{disposed=true;stopObserving();sceneRef.current?.dispose();sceneRef.current=null;};
  },[]);
  useEffect(()=>{sceneRef.current?.update({...options,...paletteRef.current});},[options]);
  return <div className={`z2-pattern-surface ${className}`} data-ready={ready} aria-hidden="true">
    {!ready && <div className="z2-pattern-poster" />}
    <div ref={containerRef} className="z2-pattern-canvas-host" />
  </div>;
}
