import { beforeEach, describe, expect, it, vi } from "vitest";
import { observePatternPalette, readPatternPalette } from "./theme";

let accent: string;
let componentAccent: string;
let scene: string;
let notify: () => void;
const observe = vi.fn();
const disconnect = vi.fn();
const root = { parentElement: null } as unknown as HTMLElement;
const host = { parentElement: root } as unknown as HTMLElement;

beforeEach(() => {
  vi.clearAllMocks();
  accent = "244 150 88";
  componentAccent = ""; scene = "30 20 16";
  vi.stubGlobal("getComputedStyle", () => ({ getPropertyValue: (name:string) => name === "--accent" ? accent : name === "--z2-accent-rgb" ? componentAccent : scene }));
  vi.stubGlobal("MutationObserver", class {
    observe = observe;
    disconnect = disconnect;
    constructor(callback: () => void) { notify = callback; }
  });
});

describe("Pattern Waves theme synchronization", () => {
  it("reads the inherited main accent rather than a service-specific palette", () => {
    expect(readPatternPalette(host).color).toEqual([244 / 255, 150 / 255, 88 / 255, 1]);
    accent = "122 152 190";
    expect(readPatternPalette(host).color).toEqual([122 / 255, 152 / 255, 190 / 255, 1]);
  });

  it("keeps valid shader uniforms if the CSS accent is missing or invalid", () => {
    for (const invalid of ["", "NaN 2 3", "999 2 3", "1 2"]) {
      accent = invalid;
      const palette = readPatternPalette(host);
      expect(palette.color).toEqual([99 / 255, 102 / 255, 241 / 255, 1]);
      expect(palette.background.every(Number.isFinite)).toBe(true);
    }
  });

  it("uses Yani's sage surface palette without inheriting its amber highlight", () => {
    accent = "232 137 46"; componentAccent = "127 168 150"; scene = "22 37 29";
    expect(readPatternPalette(host)).toEqual({color:[127/255,168/255,150/255,1],background:[22/255,37/255,29/255,1]});
  });

  it("recolors the existing scene once per palette change and releases observers", () => {
    const update = vi.fn();
    const stop = observePatternPalette(host, update);
    expect(update).toHaveBeenCalledTimes(1);
    expect(observe).toHaveBeenCalledWith(root, { attributes: true, attributeFilter: ["data-theme"] });
    notify();
    expect(update).toHaveBeenCalledTimes(1);
    accent = "122 152 190";
    notify();
    expect(update).toHaveBeenCalledTimes(2);
    scene = "17 25 34";
    notify();
    expect(update).toHaveBeenCalledTimes(3);
    expect(Object.keys(update.mock.lastCall![0]).sort()).toEqual(["background", "color"]);
    stop();
    expect(disconnect).toHaveBeenCalledTimes(1);
  });
});
