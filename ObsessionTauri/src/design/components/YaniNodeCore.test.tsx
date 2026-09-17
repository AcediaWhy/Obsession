import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { YaniNodeCore } from "./YaniNodeCore";

describe("YaniNodeCore", () => {
  it("keeps the theme preview decorative", () => {
    const markup = renderToStaticMarkup(
      <YaniNodeCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );
    expect(markup).toContain("data-yani-node-core");
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and respects telemetry priority", () => {
    const markup = renderToStaticMarkup(
      <YaniNodeCore active busy scanning alarm onClick={() => {}} />,
    );
    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="running"');
  });
});
