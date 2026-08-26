import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { YaniStoryDetails } from "./YaniStoryDetails";

describe("YaniStoryDetails", () => {
  it("keeps quiet density free of the removed paper vignette", () => {
    const markup = renderToStaticMarkup(
      <YaniStoryDetails density="quiet" depth="layered" mood="idle" />,
    );
    expect(markup).not.toContain("yani-story-details__paper");
    expect(markup).not.toContain("yani-story-details__pencil");
    expect(markup).not.toContain("WORLD KNOWS: TRUE");
  });

  it("keeps story density free of the removed Niko and pencil vignette", () => {
    const markup = renderToStaticMarkup(
      <YaniStoryDetails density="story" depth="deep" mood="active" />,
    );
    expect(markup).toContain('data-depth="deep"');
    expect(markup).toContain('data-mood="active"');
    expect(markup).not.toContain("yani-story-details__paper");
    expect(markup).not.toContain("yani-story-details__pencil");
    expect(markup).not.toContain("WORLD KNOWS: TRUE");
  });

  it("reserves World Machine glitches and pancakes for chaotic density", () => {
    const markup = renderToStaticMarkup(
      <YaniStoryDetails density="chaotic" depth="flat" mood="scanning" />,
    );
    expect(markup).toContain("WORLD KNOWS: TRUE");
    expect(markup).toContain("yani-story-details__pancake");
    expect(markup).toContain("yani-story-details__glitch");
  });
});
