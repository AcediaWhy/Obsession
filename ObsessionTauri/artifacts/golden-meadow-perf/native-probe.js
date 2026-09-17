// Temporary native WebView2 probe. Loaded only during the manual performance run.
if (window.__TAURI_INTERNALS__) {
  let last = 0, frame = 0;
  let intervals = [], longTasks = [], interactions = 0;
  const onInput = () => { interactions++; };
  for (const event of ['click', 'wheel', 'scroll']) window.addEventListener(event, onInput, {capture:true,passive:true});
  const observer = new PerformanceObserver(list => longTasks.push(...list.getEntries().map(e=>e.duration)));
  observer.observe({type:'longtask'});
  const tick = now => {
    if (!document.hidden && last) intervals.push(now-last);
    last = document.hidden ? 0 : now;
    frame = requestAnimationFrame(tick);
  };
  frame = requestAnimationFrame(tick);
  const timer = setInterval(() => {
    if(document.hidden) { intervals=[]; longTasks=[]; interactions=0; return; }
    const sorted=[...intervals].sort((a,b)=>a-b);
    const canvas=document.querySelector('canvas[data-renderer]');
    const data={at:new Date().toISOString(),theme:document.querySelector('[data-theme]')?.dataset.theme,
      screen:document.querySelector('main h1')?.textContent,interactions,frames:intervals.length,
      p50:sorted[Math.floor(sorted.length*.5)],p95:sorted[Math.floor(sorted.length*.95)],max:Math.max(0,...intervals),
      over50:intervals.filter(x=>x>50).length,longTasks,renderer:canvas?.dataset.renderer,
      surfaceBytes:Number(canvas?.dataset.surfaceBytes),scale:Number(canvas?.dataset.renderScale),
      heapBytes:performance.memory?.usedJSHeapSize};
    intervals=[];longTasks=[];interactions=0;
    void fetch('/__meadow-perf',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}).catch(()=>{});
  },2000);
  if(import.meta.hot) import.meta.hot.dispose(()=>{
    clearInterval(timer);cancelAnimationFrame(frame);observer.disconnect();
    for(const event of ['click','wheel','scroll']) window.removeEventListener(event,onInput,true);
  });
}
