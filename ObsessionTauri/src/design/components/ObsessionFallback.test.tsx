import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ObsessionFallback } from "./ObsessionFallback";

describe("ObsessionFallback", () => {
  it("preserves the optical mark, focus point and phase without external media", () => {
    const markup = renderToStaticMarkup(
      <ObsessionFallback phase="fault" screen="telegram" paused />,
    );
    expect(markup).toContain('data-testid="obsession-fallback"');
    expect(markup).toContain('data-obsession-phase="fault"');
    expect(markup).toContain("--obsession-focus-x:84%");
    expect(markup).toContain("obsession-fallback-core");
    expect(markup).not.toContain("<img");
    expect(markup).not.toContain("<video");
  });
});
