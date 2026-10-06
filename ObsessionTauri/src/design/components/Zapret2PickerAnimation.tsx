import { useEffect, useMemo, useRef, useState } from "react";
import type { Zapret2PickerProgress, Zapret2PickerResult } from "../../lib/tauri";
import { applicablePickerResult } from "../../store/zapret2StrategyStore";
import { useMotionOff, useRenderHidden } from "../render";
import { patternOptions, patternService, patternStage } from "./patternWaves/state";
import { PatternWavesSurface } from "./patternWaves/Surface";
import "../../styles/zapret2-picker-animation.css";

export function Zapret2PickerAnimation({ progress, label, cancelling, result, suppressExit=false }: {
  progress:Zapret2PickerProgress | null; label:string; cancelling:boolean;
  result?:Zapret2PickerResult; suppressExit?:boolean;
}) {
  const motionOff=useMotionOff();
  const hidden=useRenderHidden();
  const [retained,setRetained]=useState(progress);
  const [exiting,setExiting]=useState(false);
  const ref=useRef<HTMLDivElement>(null);
  const [visible,setVisible]=useState(true);
  const shown=progress ?? (!suppressExit && !motionOff && !hidden ? retained : null);
  const confirmed=applicablePickerResult(result);
  useEffect(()=>{
    setExiting(false);
    if(progress) {setRetained(progress);return;}
    if(motionOff || hidden || suppressExit || !visible) {setRetained(null);return;}
    // Let the confirmed icon settle, then fade the whole scene and ease the
    // occupied space closed. New progress cancels both timers immediately.
    const hold=confirmed ? 4200 : 650;
    const fade=window.setTimeout(()=>setExiting(true),hold);
    const remove=window.setTimeout(()=>setRetained(null),hold+1400);
    return ()=>{window.clearTimeout(fade);window.clearTimeout(remove);};
  },[progress,motionOff,hidden,suppressExit,confirmed,visible]);
  useEffect(()=>{
    if(!shown || !ref.current || typeof IntersectionObserver==="undefined")return;
    const observer=new IntersectionObserver(([entry])=>setVisible(entry.isIntersecting));
    observer.observe(ref.current);
    return ()=>observer.disconnect();
  },[!!shown]);
  const ending=!progress;
  const fading=ending && exiting;
  const stage=shown ? patternStage(shown,cancelling,ending,confirmed,result?.status==="cancelled") : "searching";
  const options=useMemo(()=>({...patternOptions(stage),service:patternService(shown?.category ?? "discord"),
    candidate:shown ? `${shown.profileId}:${shown.current}` : "",spacing:7,fadeSize:.45,transitionSeconds:4.4,maxDpr:1,motionOff,
    ...(fading ? {paused:true,interactive:false,speed:0} : {})}),
    [stage,shown?.category,shown?.profileId,shown?.current,motionOff,fading]);
  if(!shown)return null;
  const restoring=cancelling || shown.phase==="restoring";
  const caption=ending ? confirmed ? "Подбор завершён" : result?.status==="cancelled" ? "Подбор отменён" : "Проверка завершена"
    : restoring ? "Возвращаем исходный профиль…" : shown.phase==="composition" ? "Проверяем сборку…" : shown.phase==="confirmation" ? "Перепроверяем результат…" : `Подбираем ${shown.transport.toUpperCase()}…`;
  return <div ref={ref} className="z2-picker-animation" data-phase={stage} data-paused={hidden || !visible || motionOff}
    data-ending={ending} data-exiting={fading} role={ending ? undefined : "progressbar"} aria-label={ending ? undefined : `Подбор ${label}`}
    aria-valuemin={ending ? undefined : 0} aria-valuemax={ending ? undefined : shown.total || 1}
    aria-valuenow={ending ? undefined : Math.max(0,Math.min(shown.current,shown.total || 1))}>
    <div className="z2-picker-animation-content">
    <div className="z2-picker-animation-caption" role="status"><span>{caption}</span>
      <span className="tabular-nums">{ending ? "" : `${shown.current} / ${shown.total || "…"}`}</span></div>
    <div className="z2-picker-animation-scene">
      {hidden || !visible ? <div className="z2-pattern-poster" /> : <PatternWavesSurface options={options} />}
    </div>
    </div>
  </div>;
}
