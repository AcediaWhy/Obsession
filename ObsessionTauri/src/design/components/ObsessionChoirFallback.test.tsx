import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ObsessionChoirFallback } from "./ObsessionChoirFallback";

describe("ObsessionChoirFallback", () => {
  it("contains all shared curves and nine latent eyes", () => {
    const markup = renderToStaticMarkup(
      <ObsessionChoirFallback phase="idle" screen="overview" paused />,
    );
    expect(markup).toContain('class="choir-fallback-curves"');
    expect(markup.match(/<path/g)?.length).toBeGreaterThanOrEqual(34);
    expect(markup.match(/<circle/g)?.length).toBeGreaterThanOrEqual(10);
    expect(markup.match(/<ellipse/g)?.length).toBeGreaterThanOrEqual(2);
    expect(markup).toContain('class="choir-fallback-flow"');
    expect(markup).toContain('class="choir-fallback-master-blink"');
    expect(markup).toContain('data-paused="true"');
  });
});
