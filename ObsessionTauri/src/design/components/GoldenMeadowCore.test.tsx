import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useMotionOff: () => false,
  useRenderHidden: () => false,
}));
vi.mock("../useObsessionVisualPhase", () => ({
  useObsessionVisualPhase: () => "idle",
}));

import { GoldenMeadowCore } from "./GoldenMeadowCore";

describe("GoldenMeadowCore", () => {
  it("renders the layered idle cat as the hero button", () => {
    const markup = renderToStaticMarkup(<GoldenMeadowCore active={false} onClick={() => {}} />);
    expect(markup).toContain('data-black-pond-cat-sprite="true"');
    expect(markup).toContain('data-idle-renderer="layered"');
    expect(markup).toContain("black-cat-states/idle-256-still.webp");
    expect(markup).toContain('data-motion="running"');
    expect(markup).toContain("<button");
    expect(markup).not.toContain("<canvas");
  });

  it("keeps the theme tile decorative and uses the focused cat when selected", () => {
    const markup = renderToStaticMarkup(
      <GoldenMeadowCore active size={104} interactive={false} />,
    );
    expect(markup).toContain("black-cat-states/focused-128.webp");
    expect(markup).not.toContain("<button");
  });

  it("gives a fault precedence over other protection states", () => {
    const markup = renderToStaticMarkup(
      <GoldenMeadowCore active busy scanning alarm onClick={() => {}} />,
    );
    expect(markup).toContain("black-cat-states/fault-256.webp");
  });

  it("uses the retained 256 px still when a lab preview is enlarged and paused", () => {
    const markup = renderToStaticMarkup(
      <GoldenMeadowCore active={false} size={384} interactive={false} paused />,
    );
    expect(markup).toContain("black-cat-states/idle-256-still.webp");
    expect(markup).toContain('data-motion="paused"');
    expect(markup).not.toContain("idle-384");
  });
});
