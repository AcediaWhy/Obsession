import {loadSourceArt,drawSourceArt,POND_FRAME_MS} from "./design/components/axolotl/pondGifScene";
const canvas=document.querySelector<HTMLCanvasElement>("#scene")!;
const context=canvas.getContext("2d",{alpha:false})!;
const button=document.querySelector<HTMLButtonElement>("#motion")!;
const output=document.querySelector<HTMLOutputElement>("#quality")!;
const reduce=matchMedia("(prefers-reduced-motion: reduce)");
let paused=reduce.matches, animation=0, disposed=false, last=Number.NaN;
button.textContent=paused?"Продолжить":"Пауза";
button.onclick=()=>{paused=!paused;last=Number.NaN;button.textContent=paused?"Продолжить":"Пауза";};
loadSourceArt().then(scene=>{
  if(disposed) return;
  const start=performance.now();
  function tick(now:number) {
    if(disposed) return;
    const w=Math.round(innerWidth*devicePixelRatio),h=Math.round(innerHeight*devicePixelRatio);
    if(canvas.width!==w || canvas.height!==h) {
      canvas.width=w;canvas.height=h;canvas.style.width=innerWidth+"px";canvas.style.height=innerHeight+"px";last=Number.NaN;
      output.value=`1024×1024 · 16 кадров · 150 мс/кадр (6,7 FPS) · масштаб ${Math.round(Math.min(w,h)/1024*100)}% · весь кадр без повторов`;
    }
    const frame=paused?0:Math.floor(Math.max(0,now-start)/POND_FRAME_MS);
    if(frame!==last) {drawSourceArt(context,scene,w,h,frame*POND_FRAME_MS/1000,paused);last=frame;}
    animation=requestAnimationFrame(tick);
  }
  tick(start);
}).catch(()=>{output.value="Не удалось загрузить анимацию";});
if(import.meta.hot) import.meta.hot.dispose(()=>{disposed=true;cancelAnimationFrame(animation);});
