import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { THEMES } from "../store/themeStore";

const css = readFileSync(new URL("./overview.css", import.meta.url), "utf8");
const variables = (source: string) => Object.fromEntries(
  [...source.matchAll(/--note-([a-z]+):\s*(#[a-f0-9]{6})/g)].map((match) => [match[1], match[2]]),
);
const base = variables(css.slice(0, css.indexOf("[data-theme")));
const palettes = Object.fromEntries([...css.matchAll(/\[data-theme="([^"]+)"\] \.overview-board \{([^}]+)\}/g)]
  .map((match) => [match[1], { ...base, ...variables(match[2]) }]));
function luminance(hex: string) {
  return hex.slice(1).match(/../g)!.map((value) => parseInt(value, 16) / 255)
    .map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4)
    .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
}
describe("Overview note palettes", () => {
  for (const theme of THEMES) {
    it(`${theme.id} has readable text and status colors on its base surface`, () => {
      const palette = palettes[theme.id];
      expect(palette).toBeDefined();
      const paper = luminance(palette.paper);
      for (const key of ["ink", "muted", "accent", "ok", "warn"]) {
        const ink = luminance(palette[key]);
        expect((Math.max(ink, paper) + 0.05) / (Math.min(ink, paper) + 0.05), key).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
});
