import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Raindrops } from "./raindrops";

// Тесты гоняются в node-окружении: подсовываем минимальный document/canvas —
// симуляции от 2D-контекста нужны только вызовы рисования (она их не читает).
function stubDom() {
  const ctxProxy = new Proxy(
    {},
    {
      get: (target: Record<string, unknown>, prop: string) => {
        if (prop === "canvas") return null;
        if (!(prop in target)) target[prop] = vi.fn();
        return target[prop];
      },
      set: (target: Record<string, unknown>, prop: string, value: unknown) => {
        target[prop] = value;
        return true;
      },
    },
  );
  const createElement = (tag: string) => {
    if (tag !== "canvas") throw new Error(`unexpected element: ${tag}`);
    return {
      width: 0,
      height: 0,
      getContext: () => ctxProxy,
    } as unknown as HTMLCanvasElement;
  };
  vi.stubGlobal("document", { createElement });
}

// Детерминированный LCG вместо Math.random — прогоны воспроизводимы.
function seedRandom(seed: number) {
  let state = seed >>> 0;
  vi.spyOn(Math, "random").mockImplementation(() => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 4294967296;
  });
}

const sprite = {} as CanvasImageSource;

function createSim(options: ConstructorParameters<typeof Raindrops>[5] = {}) {
  return new Raindrops(1024, 768, 1, sprite, sprite, {
    maxDrops: 60,
    rainChance: 0.4,
    rainLimit: 6,
    dropletsRate: 20,
    ...options,
  });
}

describe("Raindrops (codrops port)", () => {
  beforeEach(() => {
    stubDom();
    seedRandom(42);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("spawns drops while raining and stays near the cap", () => {
    const sim = createSim();
    for (let i = 0; i < 300; i += 1) {
      sim.step(1 / 60);
      // Кап в codrops «мягкий»: createDrop сверяется с длиной списка ДО
      // вливания капель текущего кадра, так что возможен перелёт до rainLimit.
      expect(sim.drops.length).toBeLessThanOrEqual(60 + 6);
    }
    expect(sim.drops.length).toBeGreaterThan(0);
  });

  it("keeps physics finite through a freeze-sized dt", () => {
    const sim = createSim();
    for (let i = 0; i < 60; i += 1) sim.step(1 / 60);
    sim.step(0.5); // фриз: timeScale клампится в 2.0, формулы не взрываются
    for (const drop of sim.drops) {
      expect(Number.isFinite(drop.x)).toBe(true);
      expect(Number.isFinite(drop.y)).toBe(true);
      expect(Number.isFinite(drop.r)).toBe(true);
    }
  });

  it("reports wipes for moving drops so mist can be cleared", () => {
    const sim = createSim();
    let sawWipe = false;
    for (let i = 0; i < 240 && !sawWipe; i += 1) {
      sim.step(1 / 60);
      sawWipe = sim.wipes.length > 0;
      for (const wipe of sim.wipes) {
        expect(wipe.x).toBeGreaterThanOrEqual(-0.2);
        expect(wipe.x).toBeLessThanOrEqual(1.2);
        expect(wipe.radius).toBeGreaterThan(0);
      }
    }
    expect(sawWipe).toBe(true);
  });

  it("drains the sky when raining stops", () => {
    const sim = createSim();
    for (let i = 0; i < 240; i += 1) sim.step(1 / 60);
    sim.options.raining = false;
    // Без спавна популяция только падает (кто-то уезжает за низ, мелочь тает).
    let prev = sim.drops.length;
    for (let i = 0; i < 600; i += 1) {
      sim.step(1 / 60);
      expect(sim.drops.length).toBeLessThanOrEqual(prev + 0);
      prev = sim.drops.length;
    }
  });

  it("step after destroy is a no-op", () => {
    const sim = createSim();
    for (let i = 0; i < 30; i += 1) sim.step(1 / 60);
    sim.destroy();
    expect(() => sim.step(1 / 60)).not.toThrow();
    expect(sim.drops.length).toBe(0);
  });
});
