import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  CORE_HERO_MIN_SIZE: 160,
  createRenderLoop: vi.fn(),
  frameQualityScale: () => 1,
  useMotionOff: () => false,
}));

import { ObsessionChoirCore } from "./ObsessionChoirCore";

describe("ObsessionChoirCore", () => {
  it("keeps the preview decorative and outside an interactive tree", () => {
    const markup = renderToStaticMarkup(
      <ObsessionChoirCore active={false} paused interactive={false} onClick={() => {}} size={104} />,
    );
    expect(markup).toContain("data-choir-seal");
    expect(markup).toContain('data-motion="still"');
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("tabindex");
  });

  it("retains one real hero button and exposes phase state", () => {
    const markup = renderToStaticMarkup(
      <ObsessionChoirCore active={false} scanning onClick={() => {}} />,
    );
    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-phase="scanning"');
    expect(markup).toContain('data-motion="running"');
  });
});
