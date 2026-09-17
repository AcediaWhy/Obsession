import { createRainCatRig } from './labs/rainCatRig.js';
const canvas=document.querySelector('#cat');
const status=document.querySelector('#status');
const pause=document.querySelector('#pause'),neutralButton=document.querySelector('#neutral');
const strength=document.querySelector('#strength'),rain=document.querySelector('#rain');
const media=matchMedia('(prefers-reduced-motion: reduce)');
let paused=media.matches,neutral=false,time=0,last=null,raf=0,rig,disposed=false,windAt=-100,blinkAtTime=-100,state='idle',stateAt=0,lastPreview=-Infinity;
const previews=[];
const stateLabels={idle:'Отдыхает',engaging:'Собирается',scanning:'Осматривается',focused:'Сосредоточен',fault:'Испуг → настороженность'};
const events=new AbortController(),signal=events.signal;
function options(){return {strength:Number(strength.value),neutral,rain:rain.checked,windAt,blinkAtTime,state,stateAt};}
function renderAll(force=false){
  rig.render(time,options());
  if(force||time-lastPreview>=1/24){
    // Small simultaneous previews at 24fps; the large selected study stays at display rate.
    for(const preview of previews)preview.rig.render(time,{...options(),state:preview.state,stateAt:preview.state==='fault'?Math.floor(time/7)*7:0});
    lastPreview=time;
  }
}
function frame(now){
  raf=0;
  if(disposed||paused||neutral||document.hidden){last=null;return;}
  if(last!==null) time+=Math.min((now-last)/1000,0.25);
  last=now;renderAll();raf=requestAnimationFrame(frame);
}
function refresh(){
  if(!rig||disposed)return;
  pause.textContent=paused?'Продолжить':'Пауза';pause.setAttribute('aria-pressed',String(paused));
  neutralButton.textContent=neutral?'Вернуть движение':'Исходная поза';neutralButton.setAttribute('aria-pressed',String(neutral));
  status.textContent=neutral?'Исходная поза':paused?'Пауза':stateLabels[state];
  document.querySelector('#state-title').textContent=state.toUpperCase();
  canvas.setAttribute('aria-label',`Кот с зонтом: ${stateLabels[state]}`);
  for(const button of document.querySelectorAll('[data-state]'))button.setAttribute('aria-pressed',String(button.dataset.state===state));
  renderAll(true);
  if(paused||neutral||document.hidden){cancelAnimationFrame(raf);raf=0;last=null;}
  else if(!raf)raf=requestAnimationFrame(frame);
}
pause.addEventListener('click',()=>{paused=!paused;refresh();},{signal});
neutralButton.addEventListener('click',()=>{neutral=!neutral;refresh();},{signal});
document.querySelector('#wind').addEventListener('click',()=>{windAt=time;paused=neutral=false;refresh();},{signal});
document.querySelector('#blink').addEventListener('click',()=>{blinkAtTime=time;paused=neutral=false;refresh();},{signal});
for(const button of document.querySelectorAll('[data-state]'))button.addEventListener('click',()=>{
  state=button.dataset.state;stateAt=time;neutral=false;refresh();
},{signal});
document.querySelector('#replay').addEventListener('click',()=>{stateAt=time;neutral=false;paused=media.matches;refresh();},{signal});
strength.addEventListener('input',()=>{document.querySelector('#amount').value=Number(strength.value).toFixed(1).replace('.',',')+'×';refresh();},{signal});
rain.addEventListener('change',refresh,{signal});
document.querySelector('#background').addEventListener('change',e=>{document.body.dataset.background=e.target.value;},{signal});
document.addEventListener('visibilitychange',refresh,{signal});
media.addEventListener('change',()=>{paused=media.matches;refresh();},{signal});
function dispose(){disposed=true;events.abort();cancelAnimationFrame(raf);rig?.dispose();for(const preview of previews)preview.rig.dispose();}
window.addEventListener('pagehide',e=>{if(!e.persisted)dispose();},{signal});
if(import.meta.hot)import.meta.hot.dispose(dispose);
Promise.all(['body','tail','eyes','umbrella','paw','puddle'].map(async name=>{
  const image=new Image();image.src=`/lab-assets/rain-cat-user-v2/${name}.webp`;await image.decode();return [name,image];
})).then(entries=>{
  if(disposed)return;
  const images=Object.fromEntries(entries);
  rig=createRainCatRig(canvas,images);
  for(const element of document.querySelectorAll('[data-preview]'))previews.push({state:element.dataset.preview,rig:createRainCatRig(element,images)});
  refresh();
})
.catch(()=>{if(!disposed)status.textContent='Не удалось загрузить слои. Обнови страницу.';});
