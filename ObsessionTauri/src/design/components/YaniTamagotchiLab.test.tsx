import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("../render", () => ({
  useRenderActive: () => true,
}));

import { YaniTamagotchiLab } from "./YaniTamagotchiLab";

describe("YaniTamagotchiLab", () => {
  it("stays decorative in the theme preview", () => {
    const markup = renderToStaticMarkup(
      <YaniTamagotchiLab active={false} interactive={false} onClick={() => {}} size={104} />,
    );

    expect(markup).toContain("data-yani-tamagotchi-lab");
    expect(markup).toContain('data-mood="idle"');
    expect(markup).not.toContain("<button");
  });

  it("keeps one hero button and gives alarms priority", () => {
    const markup = renderToStaticMarkup(
      <YaniTamagotchiLab active busy scanning alarm onClick={() => {}} />,
    );

    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(markup).toContain('data-mood="alarm"');
    expect(markup).toContain('data-motion="running"');
  });
});
