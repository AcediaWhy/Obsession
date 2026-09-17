import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

const renderState = vi.hoisted(() => ({ active: true }));
vi.mock("../render", () => ({
  useRenderActive: () => renderState.active,
}));

import { RainBenchCore } from "./RainBenchCore";

describe("RainBenchCore", () => {
  it("renders the approved layered cat without a nested preview button", () => {
    const markup = renderToStaticMarkup(
      <RainBenchCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-rain-bench-core");
    expect(markup).toContain('data-placement="preview"');
    expect(markup).toContain("data-rain-cat-sprite");
    expect(markup).toContain("rain-cat-user-v2/body.webp");
    expect(markup).toContain('data-puddle="false"');
    expect(markup).not.toContain("rain-cat-user-v2/still.webp");
    expect(markup).not.toContain("rain-cat-user-v2/puddle.webp");
    expect(markup.match(/<img/g)).toHaveLength(5);
    expect(markup).not.toContain("rain-umbrella-sprite-lab");
    expect(markup).not.toContain("rain-bench-sprite-lab");
    expect(markup).toContain('data-phase="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <RainBenchCore active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-placement="hero"');
    expect(markup).toContain('data-phase="fault"');
    expect(markup).toContain('data-motion="running"');
    expect(markup).toContain("Отключить защиту");
    expect(markup).toContain('disabled=""');
  });

  it.each([
    [{active:false},'idle'],
    [{active:true},'focused'],
    [{active:true,busy:true},'engaging'],
    [{active:true,busy:true,scanning:true},'scanning'],
    [{active:false,alarm:true},'fault'],
  ] as const)("maps protection signals %j to %s",(props,phase)=>{
    const markup=renderToStaticMarkup(<RainBenchCore {...props} onClick={()=>{}} />);
    expect(markup).toContain(`data-phase="${phase}"`);
  });

  it("stops motion when explicitly paused or rendering is inactive",()=>{
    expect(renderToStaticMarkup(<RainBenchCore active paused onClick={()=>{}} />)).toContain('data-motion="still"');
    renderState.active=false;
    try {
      expect(renderToStaticMarkup(<RainBenchCore active onClick={()=>{}} />)).toContain('data-motion="still"');
    } finally { renderState.active=true; }
  });
});
