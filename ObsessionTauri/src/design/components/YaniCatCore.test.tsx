import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { YaniCatCore } from "./YaniCatCore";

describe("YaniCatCore", () => {
  it("renders a standalone decorative cat in theme previews", () => {
    const markup = renderToStaticMarkup(
      <YaniCatCore active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-yani-cat-core");
    expect(markup).toContain('data-presentation="standalone"');
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("data-yani-tamagotchi-lab");
    expect(markup).not.toContain("cigarette");
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <YaniCatCore active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="running"');
  });
});
