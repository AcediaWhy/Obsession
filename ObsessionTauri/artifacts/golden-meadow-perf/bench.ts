import { createGoldenMeadow } from "./before.js";
import { createGoldenMeadowRenderer } from "../../src/labs/goldenMeadowRenderer";

const stage = document.querySelector<HTMLDivElement>("#stage")!;
const panels = [...document.querySelectorAll<HTMLDivElement>(".panel")];
for (const panel of panels) panel.innerHTML = Array.from({length:80}, (_, i) => `<div class="row"><b>Section ${i}</b><p>Golden Meadow — scrolling content, shadows, transparent panels</p></div>`).join("");
const status = document.querySelector("#status")!;
const results = document.querySelector("#results")!;
const nextFrame = () => new Promise<number>(resolve => requestAnimationFrame(resolve));
const percentile = (values: number[], fraction: number) => [...values].sort((a,b)=>a-b)[Math.floor((values.length-1)*fraction)] ?? 0;

async function measure(mode: "before" | "worker") {
  status.textContent = `Running ${mode}`;
  const canvas = document.createElement("canvas");
  stage.append(canvas);
  const gaps: number[] = [], steady: number[] = [], costs: number[] = [], longTasks: number[] = [];
  const observer = new PerformanceObserver(list => longTasks.push(...list.getEntries().map(e=>e.duration)));
  observer.observe({ type: "longtask", buffered: false });
  await nextFrame();
  const started = performance.now();
  let last = started, nextDraw = started, frame = 0, error = "";
  let renderer: {render(time:number): void; dispose():void};
  const done = new Promise<void>(resolve => {
    const tick = (now: number) => {
      const elapsed = now-started;
      gaps.push(now-last);
      if(elapsed>1500) steady.push(now-last);
      last=now;
      panels.forEach((panel,i) => {
        panel.scrollTop = (elapsed * .5 + i * 500) % (panel.scrollHeight-panel.clientHeight);
        panel.style.transform = `translateY(${Math.sin(elapsed/300+i)*6}px)`;
      });
      if(now>=nextDraw) {
        const before=performance.now();
        renderer?.render(elapsed/1000);
        costs.push(performance.now()-before);
        nextDraw=now+1000/30-1;
      }
      if(elapsed<8000) frame=requestAnimationFrame(tick); else resolve();
    };
    frame=requestAnimationFrame(tick);
  });
  const setupStart=performance.now();
  renderer=mode === "before" ? createGoldenMeadow(canvas) : createGoldenMeadowRenderer(canvas, message=>{error=message;});
  const setupMainMs=performance.now()-setupStart;
  await done;
  cancelAnimationFrame(frame);
  longTasks.push(...observer.takeRecords().map(e=>e.duration));
  observer.disconnect();
  const report={mode,error,viewport:[innerWidth,innerHeight,devicePixelRatio],setupMainMs,
    workerBuildMs:Number(canvas.dataset.buildMs),scale:Number(canvas.dataset.renderScale),
    surfaceMiB:Number(canvas.dataset.surfaceBytes)/1048576,
    uiFrames:steady.length,p95GapMs:percentile(steady,.95),p99GapMs:percentile(steady,.99),maxSteadyGapMs:Math.max(...steady),
    maxIncludingSetupMs:Math.max(...gaps),gapsOver50ms:steady.filter(x=>x>50).length,
    p95MainRenderMs:percentile(costs,.95),longTasksMs:longTasks};
  renderer.dispose();
  if(mode === "before") canvas.width=canvas.height=0;
  canvas.remove();
  return report;
}

document.querySelector<HTMLButtonElement>("#run")!.onclick=async(event)=>{
  const button=event.currentTarget as HTMLButtonElement;
  button.disabled=true;
  results.textContent="";
  const before=await measure("before");
  results.textContent=JSON.stringify([before],null,2);
  await new Promise(resolve=>setTimeout(resolve,500));
  const worker=await measure("worker");
  results.textContent=JSON.stringify([before,worker],null,2);
  status.textContent="Complete";
  button.disabled=false;
};
