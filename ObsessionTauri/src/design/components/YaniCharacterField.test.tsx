import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("./YaniCharacterScene", () => ({
  YaniCharacterScene: () => <canvas data-yani-character-scene="field" />,
}));

import { YaniCharacterField, yaniCharacterMood } from "./YaniCharacterField";

describe("YaniCharacterField", () => {
  it("maps application phases to character moods", () => {
    expect(yaniCharacterMood("idle")).toBe("idle");
    expect(yaniCharacterMood("engaging")).toBe("busy");
    expect(yaniCharacterMood("scanning")).toBe("scanning");
    expect(yaniCharacterMood("focused")).toBe("active");
    expect(yaniCharacterMood("fault")).toBe("alarm");
  });

  it("renders a lightweight mint fallback without mounting WebGL", () => {
    const markup = renderToStaticMarkup(
      <YaniCharacterField phase="focused" paused forceFallback />,
    );
    expect(markup).toContain('data-mood="active"');
    expect(markup).toContain('data-model="fallback"');
    expect(markup).toContain('data-story="off"');
    expect(markup).toContain("yani-character-field__room-depth");
    expect(markup).toContain("yani-character-field__tail-echo");
    expect(markup).not.toContain("yani-character-field__ear-echo");
    expect(markup).toContain("yani-character-field__purr-waves");
    expect(markup).toContain("yani-character-field__cat-scribble");
    expect(markup).toContain("EAR L / LISTENING");
    expect(markup).toContain("PURR / 07Hz");
    expect(markup).toContain("OBS // YANI · 03:17");
    expect(markup).toContain("yani-character-field__foreground-depth");
    expect(markup).not.toContain("yani-character-field__floor-line");
    expect(markup).not.toContain("data-yani-character-scene");
    expect(markup).not.toContain("yani-character-field__contact-shadow");
    expect(markup).not.toContain("yani-character-field__cigarette");
    expect(markup).not.toContain("yani-story-details");
  });

  it("mounts the optional story layer without changing the production default", () => {
    const markup = renderToStaticMarkup(
      <YaniCharacterField storyDensity="story" storyDepth="deep" />,
    );
    expect(markup).toContain('data-story="story"');
    expect(markup).toContain('data-depth="deep"');
    expect(markup).toContain('class="yani-story-details"');
    expect(markup).not.toContain("niko-pencil-portrait-v2.webp");
    expect(markup).not.toContain("yani-story-details__pencil");
    expect(markup).not.toContain("WORLD KNOWS: TRUE");
  });
});
