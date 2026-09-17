import {describe,it,expect,vi} from "vitest";
import {drawSourceArt,type PondScene} from "./pondGifScene";

function fixture() {
  const frames=Array.from({length:16},()=>({})) as HTMLImageElement[];
  const drawImage=vi.fn();
  const context={setTransform:vi.fn(),fillRect:vi.fn(),drawImage} as unknown as CanvasRenderingContext2D;
  return {scene:{frames} satisfies PondScene,context,drawImage};
}
describe("pond GIF playback",()=>{
  it("draws a valid first frame when an animation timestamp predates initialization",()=>{
    const {scene,context,drawImage}=fixture();
    drawSourceArt(context,scene,1280,720,-.015);
    expect(drawImage.mock.calls[0][0]).toBe(scene.frames[0]);
  });
  it("advances at source timing and wraps the complete 2.4 second loop",()=>{
    const {scene,context,drawImage}=fixture();
    for(const time of [0,.15,1.2,2.4]) drawSourceArt(context,scene,1280,720,time);
    expect(drawImage.mock.calls.map(call=>call[0])).toEqual([scene.frames[0],scene.frames[1],scene.frames[8],scene.frames[0]]);
  });
  it("covers a wide viewport without stretching the square source",()=>{
    const {scene,context,drawImage}=fixture();
    drawSourceArt(context,scene,1920,1080,1.2,true);
    expect(drawImage).toHaveBeenCalledTimes(1);
    expect(drawImage).toHaveBeenCalledWith(scene.frames[0],0,224,1024,576,0,0,1920,1080);
  });
});
