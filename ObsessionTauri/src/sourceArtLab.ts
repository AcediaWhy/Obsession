import {drawSourceArt,loadSourceArt} from "./design/components/axolotl/sourceArtScene";
const canvas=document.querySelector<HTMLCanvasElement>("#scene")!;
const context=canvas.getContext("2d",{alpha:false})!;
const motion=document.querySelector<HTMLButtonElement>("#motion")!;
const blink=document.querySelector<HTMLButtonElement>("#blink")!;
const quality=document.querySelector<HTMLOutputElement>("#quality")!;
let paused=matchMedia("(prefers-reduced-motion: reduce)").matches, forced=false, animation=0;
motion.textContent=paused?"Продолжить":"Пауза";
motion.onclick=()=>{paused=!paused;forced=false;motion.textContent=paused?"Продолжить":"Пауза";};
blink.onclick=()=>{forced=!forced;blink.textContent=forced?"Открыть глаза":"Проверить моргание";};
loadSourceArt().then(scene=>{
  const start=performance.now();
  let last=Number.NaN;
  function tick(now:number) {
    const ratio=devicePixelRatio || 1,w=Math.round(innerWidth*ratio),h=Math.round(innerHeight*ratio);
    if(canvas.width!==w || canvas.height!==h) {
      canvas.width=w;canvas.height=h;canvas.style.width=innerWidth+"px";canvas.style.height=innerHeight+"px";last=Number.NaN;
      const scale=Math.max(w/2400,h/1792);
      quality.value=`Исходник 2400×1792 · экран ${w}×${h} · ${Math.round(scale*100)}% · без искажения пропорций`;
    }
    const frame=forced?-2:paused?-1:Math.floor((now-start)/66);
    if(frame!==last) {
      drawSourceArt(context,scene,w,h,forced?4.23:frame*.066,paused&&!forced);
      last=frame;
    }
    animation=requestAnimationFrame(tick);
  }
  tick(start);
}).catch(()=>{quality.value="Не удалось загрузить исходный арт";});
if(import.meta.hot) import.meta.hot.dispose(()=>cancelAnimationFrame(animation));
