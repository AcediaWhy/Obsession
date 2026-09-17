import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  CORE_HERO_MIN_SIZE: 160,
  createRenderLoop: vi.fn(),
  frameQualityScale: () => 1,
  useRenderActive: () => false,
}));

import { YaniNekoCore } from "./YaniNekoCore";

describe("YaniNekoCore", () => {
  it("keeps its preview decorative without creating a nested button", () => {
    const markup = renderToStaticMarkup(
      <YaniNekoCore active={false} paused interactive={false} onClick={() => {}} size={104} />,
    );
    expect(markup).toContain("data-yani-core");
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("tabindex");
  });

  it("retains one real hero button and exposes telemetry priority", () => {
    const markup = renderToStaticMarkup(
      <YaniNekoCore active busy scanning alarm onClick={() => {}} />,
    );
    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="running"');
  });
});
