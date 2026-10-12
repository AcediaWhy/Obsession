import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ThemeNavIcon } from "./ThemeNavIcon";
import { HalloweenIcon } from "./HalloweenIcon";
import { screenVariants } from "../screenTransition";

describe("navigation polish", () => {
  it("keeps catnap toes symmetric without nested rotation pivots", () => {
    const svg = renderToStaticMarkup(<ThemeNavIcon theme="catnap" item="profiles" />);
    expect(svg.match(/<ellipse/g)).toHaveLength(4);
    expect(svg).not.toContain('transform="rotate');
    for (const x of [6, 12, 20, 26]) expect(svg).toContain(`cx="${x}"`);
  });
  it("keeps bat wings independent from its centered face", () => {
    const svg = renderToStaticMarkup(<HalloweenIcon item="telegram" />);
    expect(svg).toContain('class="halloween-wing-left"');
    expect(svg).toContain('class="halloween-wing-right"');
    expect(svg).toContain('cx="14" cy="17"');
    expect(svg).toContain('cx="18" cy="17"');
  });
  it("keeps the route stable and interactive while its contents take focus", () => {
    const center = screenVariants.center as Record<string, unknown>;
    expect(center.transition).toMatchObject({ duration: 0 });
    expect(center.pointerEvents).toBe("auto");
    expect(center["--route-presence"]).toBe(1);
    for (const property of ["y", "scale", "opacity", "filter"]) expect(center).not.toHaveProperty(property);
  });
});
