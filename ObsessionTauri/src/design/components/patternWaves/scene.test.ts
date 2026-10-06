import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPatternWaves } from "./scene";

const harness=vi.hoisted(()=>({
  render:vi.fn(),lose:vi.fn(),removeProgram:vi.fn(),removeGeometry:vi.fn(),deleteTexture:vi.fn(),
  loop:{start:vi.fn(),stop:vi.fn(),setPaused:vi.fn(),invalidate:vi.fn(),dispose:vi.fn()},
  draw:null as null | ((dt:number,now:number)=>void), intersection:null as null | ((entries:{isIntersecting:boolean}[])=>void),
  resize:null as null | (()=>void),
  quality:null as null | ((tier:string)=>void),
  canvas:null as unknown as HTMLCanvasElement, windowEvents:new Map<string,(event:unknown)=>void>(),
  canvasEvents:new Map<string,(event:unknown)=>void>(),webgl2:true,link:true,
}));
vi.mock("../../render",()=>({createRenderLoop:(draw:typeof harness.draw,options:{onQualityChange:(tier:string)=>void})=>{
  harness.draw=draw; harness.quality=options.onQualityChange; return harness.loop;
}}));
vi.mock("ogl",()=>{
  class Texture { texture={}; constructor(){} }
  class Geometry { remove=harness.removeGeometry; constructor(){} }
  class Program { program={}; remove=harness.removeProgram; setBlendFunc=vi.fn(); uniforms:Record<string,{value:unknown}>;
    constructor(_gl:unknown,options:{uniforms:Record<string,{value:unknown}>}){this.uniforms=options.uniforms;} }
  return {Texture,Geometry,Triangle:Geometry,Program,
    Mesh:class {program:Program;constructor(_gl:unknown,options:{program:Program}){this.program=options.program;}},
    RenderTarget:class {width:number;height:number;texture=new Texture();buffer={};constructor(_gl:unknown,{width,height}:{width:number;height:number}){this.width=width;this.height=height;}},
    Renderer:class {
      isWebgl2=harness.webgl2;dpr=1;render=harness.render;bindFramebuffer=vi.fn();
      gl={canvas:harness.canvas,clearColor:vi.fn(),deleteFramebuffer:vi.fn(),deleteTexture:harness.deleteTexture,
        getExtension:(name:string)=>name==="WEBGL_lose_context"?{loseContext:harness.lose}:null,
        getProgramParameter:()=>harness.link,ONE:1,ONE_MINUS_SRC_ALPHA:2,LINK_STATUS:3};
      setSize(width:number,height:number){harness.canvas.width=Math.round(width*this.dpr);harness.canvas.height=Math.round(height*this.dpr);}
    },
  };
});
function container(width=360,height=144) {
  return {clientWidth:width,clientHeight:height,appendChild:vi.fn(),
    getBoundingClientRect:()=>({left:0,top:0,width,height})} as unknown as HTMLElement;
}
beforeEach(()=>{
  vi.clearAllMocks();harness.windowEvents.clear();harness.canvasEvents.clear();harness.webgl2=true;harness.link=true;
  harness.canvas={width:0,height:0,style:{},dataset:{},setAttribute:vi.fn(),parentNode:null,
    addEventListener:(name:string,callback:(event:unknown)=>void)=>harness.canvasEvents.set(name,callback),
    removeEventListener:(name:string)=>harness.canvasEvents.delete(name)} as unknown as HTMLCanvasElement;
  vi.stubGlobal("window",{devicePixelRatio:3,addEventListener:(name:string,callback:(event:unknown)=>void)=>harness.windowEvents.set(name,callback),
    removeEventListener:(name:string)=>harness.windowEvents.delete(name)});
  vi.stubGlobal("document",{hidden:false,addEventListener:vi.fn(),removeEventListener:vi.fn(),createElement:()=>({getContext:()=>null})});
  vi.stubGlobal("ResizeObserver",class {constructor(callback:()=>void){harness.resize=callback;}observe(){}disconnect(){}});
  vi.stubGlobal("IntersectionObserver",class {constructor(callback:typeof harness.intersection){harness.intersection=callback;}observe(){}disconnect(){}});
});
describe("pattern canvas lifecycle and idle cost",()=>{
  it("does not slow the animation clock when frequent pointer events arrive between frames",()=>{
    const clock=vi.spyOn(performance,"now").mockReturnValue(0);
    const scene=createPatternWaves(container(),{intro:false});
    harness.draw!(.02,20);
    for(let now=22;now<=38;now+=2) {
      clock.mockReturnValue(now);
      harness.windowEvents.get("pointermove")!({clientX:now+100,clientY:70});
    }
    harness.draw!(.02,40);
    const field=harness.render.mock.calls.find(([call])=>call.scene.program.uniforms.uTime)![0].scene.program;
    expect(field.uniforms.uTime.value).toBeCloseTo(.04*.35,8);
    scene.dispose(); clock.mockRestore();
  });
  it("keeps the particle grid stable across adaptive backing-store quality changes",()=>{
    const scene=createPatternWaves(container(),{intro:false});
    harness.draw!(1/60,performance.now()+17);
    const mark=harness.render.mock.lastCall![0].scene.program;
    const grid=[...mark.uniforms.uGrid.value];
    const highWidth=harness.canvas.width;
    harness.quality!("low"); harness.draw!(1/60,performance.now()+34);
    expect(harness.canvas.width).toBeLessThan(highWidth);
    expect(mark.uniforms.uGrid.value).toEqual(grid);
    scene.dispose();
  });
  it("pauses a zero-size layout and resumes after its real size arrives",()=>{
    const host=container(0,0);
    const scene=createPatternWaves(host,{morph:1,motionOff:true,intro:false});
    harness.draw!(1/60,performance.now()+17);
    expect(harness.render).not.toHaveBeenCalled(); expect(harness.loop.setPaused).toHaveBeenLastCalledWith(true);
    Object.assign(host,{clientWidth:360,clientHeight:144});
    scene.update({morph:0}); harness.resize!(); harness.draw!(1/60,performance.now()+34);
    expect(harness.render).toHaveBeenCalled(); scene.dispose();
  });
  it("keeps valid morph destinations when a tiny or empty mask has no sampled points",()=>{
    vi.stubGlobal("Path2D",class {});
    vi.stubGlobal("document",{hidden:false,addEventListener:vi.fn(),removeEventListener:vi.fn(),createElement:()=>({getContext:()=>({
      scale:vi.fn(),fill:vi.fn(),beginPath:vi.fn(),arc:vi.fn(),fillText:vi.fn(),getImageData:()=>({data:new Uint8ClampedArray(256*256*4)})
    })})});
    const scene=createPatternWaves(container(2,2),{morph:1,motionOff:true,intro:false});
    expect(()=>harness.draw!(1/60,performance.now()+17)).not.toThrow();
    expect(harness.render).toHaveBeenCalled(); scene.dispose();
  });
  it("recolors a paused scene without rebuilding its programs",()=>{
    const scene=createPatternWaves(container(),{paused:true,intro:false});
    harness.draw!(1/60,performance.now()+17);
    const programs=harness.render.mock.calls.map(([call])=>call.scene.program);
    const color=[244/255,150/255,88/255,1],background=[.08,.06,.04,1];
    scene.update({color,background});
    harness.draw!(1/60,performance.now()+34);
    const mark=harness.render.mock.lastCall![0].scene.program;
    expect(mark).toBe(programs[programs.length-1]);
    expect(mark.uniforms.uColor.value).toEqual(color.slice(0,3));
    expect(mark.uniforms.uBackground.value).toEqual(background);
    expect(harness.loop.setPaused).toHaveBeenLastCalledWith(true);
    scene.dispose();
  });
  it("bounds the backing store on a large high-DPI surface",()=>{
    const scene=createPatternWaves(container(2000,1000));
    expect(harness.canvas.width).toBeLessThanOrEqual(720);
    expect(harness.canvas.height).toBeLessThanOrEqual(320);
    scene.dispose();
  });
  it("pauses after an idle draw and ignores pointers outside the card",()=>{
    const scene=createPatternWaves(container(),{paused:true,intro:false});
    harness.draw!(1/60,performance.now()+17);
    expect(harness.loop.setPaused).toHaveBeenLastCalledWith(true);
    vi.clearAllMocks();
    harness.windowEvents.get("pointermove")!({clientX:900,clientY:900});
    expect(harness.loop.start).not.toHaveBeenCalled();
    scene.dispose();
  });
  it("stops offscreen frames and releases every program exactly once",()=>{
    const scene=createPatternWaves(container());
    harness.intersection!([{isIntersecting:false}]);
    expect(harness.loop.stop).toHaveBeenCalled();
    scene.dispose();scene.dispose();harness.draw!(1/60,performance.now()+30);scene.update({morph:1});
    expect(harness.loop.dispose).toHaveBeenCalledTimes(1);
    expect(harness.removeProgram).toHaveBeenCalledTimes(4);
    expect(harness.lose).toHaveBeenCalledTimes(1);
    expect(harness.render).not.toHaveBeenCalled();
    expect(harness.windowEvents.size).toBe(0);
  });
  it("falls back after context loss without keeping an active loop",()=>{
    const unavailable=vi.fn();const scene=createPatternWaves(container(),{onUnavailable:unavailable});
    harness.canvasEvents.get("webglcontextlost")!({preventDefault:vi.fn()});
    expect(unavailable).toHaveBeenCalledTimes(1);expect(harness.loop.dispose).toHaveBeenCalledTimes(1);
    scene.dispose();expect(harness.lose).toHaveBeenCalledTimes(1);
  });
  it("cleans up a failed shader before the React fallback is shown",()=>{
    harness.link=false;
    expect(()=>createPatternWaves(container())).toThrow("Pattern shader could not link");
    expect(harness.loop.dispose).toHaveBeenCalledTimes(1);expect(harness.lose).toHaveBeenCalledTimes(1);
  });
});
