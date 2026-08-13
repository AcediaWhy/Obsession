import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  createRenderLoop: () => {
    throw new Error("render loop is not used by RainBackdrop");
  },
  useMotionOff: () => false,
}));

import { RainBackdrop } from "./RainFallback";

describe("RainBackdrop", () => {
  it("does not load a photographic fallback", () => {
    const markup = renderToStaticMarkup(<RainBackdrop />);

    expect(markup).not.toContain("<img");
    expect(markup).not.toContain("poster.jpg");
    expect(markup).not.toContain("world.jpg");
  });
});
