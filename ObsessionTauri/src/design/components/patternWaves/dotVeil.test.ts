import { beforeEach, describe, expect, it, vi } from "vitest";
import { createDotVeil } from "./dotVeil";

const h=vi.hoisted(()=>({
  draw:null as null | ((dt:number,now:number)=>void),
  intersection:null as null | ((entries:{isIntersecting:boolean}[])=>void),
  palette:null as null | ((value:{color:number[];background:number[]})=>void),
  loop:{start:vi.fn(),stop:vi.fn(),setPaused:vi.fn(),invalidate:vi.fn(),dispose:vi.fn()},
  ctx:{setTransform:vi.fn(),clearRect:vi.fn(),beginPath:vi.fn(),arc:vi.fn(),fill:vi.fn(),globalAlpha:1,fillStyle:""},
  getContext:vi.fn(),canvas:{width:0,height:0,dataset:{} as Record<string,string>,setAttribute:vi.fn(),remove:vi.fn()},
  events:new Map<string,(event:unknown)=>void>(),windowEvents:new Map<string,(event:unknown)=>void>(),
  stopPalette:vi.fn(),
}));
vi.mock("../../render",()=>({createRenderLoop:(draw:typeof h.draw)=>{h.draw=draw;return h.loop;}}));
vi.mock("./theme",()=>({observePatternPalette:(_host:unknown,update:NonNullable<typeof h.palette>)=>{
  h.palette=update; update({color:[.5,.7,.6,1],background:[.1,.2,.1,1]}); return h.stopPalette;
}}));
const host=()=>({clientWidth:320,clientHeight:160,parentElement:null,appendChild:vi.fn(),closest:()=>null,
  getBoundingClientRect:()=>({left:0,top:0,width:320,height:160}),
  addEventListener:(name:string,callback:(event:unknown)=>void)=>h.events.set(name,callback),
  removeEventListener:(name:string)=>h.events.delete(name),
}) as unknown as HTMLElement;
const settings={interactive:true,motionOff:false,hidden:false};
beforeEach(()=>{
  vi.clearAllMocks(); h.events.clear(); h.windowEvents.clear(); h.canvas.dataset={};
  h.getContext.mockImplementation((kind:string)=>kind==="2d"?h.ctx:null);
  vi.stubGlobal("document",{hidden:false,createElement:()=>({...h.canvas,dataset:h.canvas.dataset,getContext:h.getContext})});
  vi.stubGlobal("window",{devicePixelRatio:3,
    addEventListener:(name:string,callback:(event:unknown)=>void)=>h.windowEvents.set(name,callback),
    removeEventListener:(name:string)=>h.windowEvents.delete(name)});
  vi.stubGlobal("ResizeObserver",class {observe(){}disconnect(){}});
  vi.stubGlobal("IntersectionObserver",class {constructor(callback:typeof h.intersection){h.intersection=callback;}observe(){}disconnect(){}});
});
describe("quiet card veil",()=>{
  it("allocates only 2D artwork and pauses immediately after drawing the idle view",()=>{
    const scene=createDotVeil(host(),settings);
    h.draw!(1/60,performance.now()+17);
    expect(h.getContext).toHaveBeenCalledOnce(); expect(h.getContext).toHaveBeenCalledWith("2d",{alpha:true});
    expect(h.loop.setPaused).toHaveBeenLastCalledWith(true);
    expect(h.ctx.arc.mock.calls.length).toBeLessThanOrEqual(1400);
    scene.dispose(); scene.dispose();
    expect(h.loop.dispose).toHaveBeenCalledOnce(); expect(h.stopPalette).toHaveBeenCalledOnce(); expect(h.events.size).toBe(0);
  });
  it("eases the pointer lens and sleeps again both under a still pointer and after leaving",()=>{
    const clock=vi.spyOn(performance,"now").mockReturnValue(0);
    const scene=createDotVeil(host(),settings);
    h.draw!(.02,20);
    clock.mockReturnValue(20); h.events.get("pointermove")!({clientX:160,clientY:80});
    h.draw!(.02,40);
    expect(Number(h.canvas.dataset.hover)).toBeGreaterThan(0);
    expect(Number(h.canvas.dataset.hover)).toBeLessThan(.2);
    for(let now=60;now<1800;now+=20)h.draw!(.02,now);
    expect(h.loop.setPaused).toHaveBeenLastCalledWith(true);
    clock.mockReturnValue(1800); h.events.get("pointerleave")!({});
    for(let now=1820;now<3600;now+=20)h.draw!(.02,now);
    expect(h.canvas.dataset.hover).toBe("0.000"); expect(h.loop.setPaused).toHaveBeenLastCalledWith(true);
    scene.dispose(); clock.mockRestore();
  });
  it("stops offscreen or hidden work and ignores mouse input when motion is disabled",()=>{
    const scene=createDotVeil(host(),settings);
    h.intersection!([{isIntersecting:false}]); expect(h.loop.stop).toHaveBeenCalled();
    vi.clearAllMocks(); h.events.get("pointermove")!({clientX:160,clientY:80}); expect(h.loop.start).not.toHaveBeenCalled();
    h.intersection!([{isIntersecting:true}]); scene.update({...settings,motionOff:true});
    vi.clearAllMocks(); h.events.get("pointermove")!({clientX:160,clientY:80}); expect(h.loop.start).not.toHaveBeenCalled();
    scene.update({...settings,hidden:true}); expect(h.loop.stop).toHaveBeenCalled(); scene.dispose();
  });
});
