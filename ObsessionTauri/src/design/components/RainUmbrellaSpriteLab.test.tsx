import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RainUmbrellaSpriteLab, rainUmbrellaMoodForPhase } from "./RainUmbrellaSpriteLab";

describe("RainUmbrellaSpriteLab", () => {
  it("maps protection phases onto umbrella moods", () => {
    expect(rainUmbrellaMoodForPhase("idle")).toBe("idle");
    expect(rainUmbrellaMoodForPhase("engaging")).toBe("busy");
    expect(rainUmbrellaMoodForPhase("scanning")).toBe("scanning");
    expect(rainUmbrellaMoodForPhase("focused")).toBe("active");
    expect(rainUmbrellaMoodForPhase("fault")).toBe("alarm");
  });

  it("renders an edge-free hero sprite with its paper boat", () => {
    const markup = renderToStaticMarkup(<RainUmbrellaSpriteLab phase="idle" paused size={240} />);

    expect(markup).toContain('data-detail="hero"');
    expect(markup).toContain("rain-umbrella-sprite-lab__umbrella");
    expect(markup).toContain("rain-umbrella-sprite-lab__boat");
    expect(markup).not.toContain("scene-bg");
  });

  it("uses the compact grid for a real theme preview", () => {
    const markup = renderToStaticMarkup(<RainUmbrellaSpriteLab phase="fault" size={104} />);

    expect(markup).toContain('viewBox="0 0 52 52"');
    expect(markup).toContain('data-detail="base"');
    expect(markup).toContain('data-mood="alarm"');
  });
});
