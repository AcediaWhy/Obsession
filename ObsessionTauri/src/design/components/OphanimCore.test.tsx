import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { OphanimCore } from "./OphanimCore";

describe("OphanimCore", () => {
  it("renders the compact pixel cat without a nested preview button", () => {
    const markup = renderToStaticMarkup(
      <OphanimCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-ophanim-cat-core");
    expect(markup).toContain("ophanim-cat-sprite-lab");
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-phase="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <OphanimCore active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-phase="fault"');
  });

  it("maps scanning and active signals to their pixel states", () => {
    const scanningMarkup = renderToStaticMarkup(
      <OphanimCore active={false} scanning onClick={() => {}} />,
    );
    const activeMarkup = renderToStaticMarkup(
      <OphanimCore active onClick={() => {}} />,
    );

    expect(scanningMarkup).toContain('data-phase="scanning"');
    expect(scanningMarkup).toContain('data-mood="scanning"');
    expect(activeMarkup).toContain('data-phase="focused"');
    expect(activeMarkup).toContain('data-mood="active"');
  });
});
