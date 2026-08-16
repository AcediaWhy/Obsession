import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  CORE_HERO_MIN_SIZE: 160,
  createRenderLoop: vi.fn(),
  frameQualityScale: () => 1,
}));
vi.mock("../pointerBus", () => ({ subscribePointerFrame: vi.fn() }));

import { ObsessionCore } from "./ObsessionCore";

describe("ObsessionCore", () => {
  it("keeps a decorative preview outside the interactive tree", () => {
    const markup = renderToStaticMarkup(
      <ObsessionCore
        active
        scanning
        paused
        interactive={false}
        onClick={() => {}}
        size={104}
      />,
    );
    expect(markup).toContain('data-obsession-core="true"');
    expect(markup).toContain('data-phase="scanning"');
    expect(markup).toContain('data-motion="still"');
    expect(markup).toContain("<canvas");
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("tabindex");
  });

  it("retains a single real button in hero mode", () => {
    const markup = renderToStaticMarkup(
      <ObsessionCore active={false} onClick={() => {}} />,
    );
    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-phase="idle"');
  });

  it("exposes the live canvas state while rendering is active", () => {
    const markup = renderToStaticMarkup(
      <ObsessionCore active scanning onClick={() => {}} />,
    );

    expect(markup).toContain('data-motion="running"');
    expect(markup).toContain('data-phase="scanning"');
  });
});
