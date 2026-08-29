import { describe, expect, it } from "vitest";

import { PersistentGlSession } from "./persistentGlSession";

type FakePipeline = { id: number; broken: boolean; canvas: unknown };

// environment: node — DOM недоступен, канвас инъекцией (как в проде —
// document.createElement, в тестах — фейк).
function makeCanvas(tag: string): HTMLCanvasElement {
  return { tagName: "CANVAS", dataset: { tag } } as unknown as HTMLCanvasElement;
}

function makeSession(pipelines: FakePipeline[], canvases: HTMLCanvasElement[]) {
  return new PersistentGlSession<FakePipeline>(
    (canvas) => {
      const pipeline: FakePipeline = {
        id: pipelines.length + 1,
        broken: false,
        canvas,
      };
      pipelines.push(pipeline);
      return pipeline;
    },
    (pipeline) => pipeline.broken,
    () => {
      const canvas = makeCanvas(`c${canvases.length + 1}`);
      canvases.push(canvas);
      return canvas;
    },
  );
}

describe("PersistentGlSession", () => {
  it("reuses the same canvas and pipeline across acquire() calls", () => {
    const pipelines: FakePipeline[] = [];
    const canvases: HTMLCanvasElement[] = [];
    const session = makeSession(pipelines, canvases);

    const first = session.acquire();
    const second = session.acquire();

    expect(second).toBe(first);
    expect(pipelines).toHaveLength(1);
    expect(canvases).toHaveLength(1);
    expect(first.pipeline.canvas).toBe(first.canvas);
  });

  it("recreates canvas+pipeline when the previous session went broken", () => {
    const pipelines: FakePipeline[] = [];
    const canvases: HTMLCanvasElement[] = [];
    const session = makeSession(pipelines, canvases);

    const dead = session.acquire();
    dead.pipeline.broken = true;

    const next = session.acquire();

    expect(next).not.toBe(dead);
    expect(next.pipeline.id).toBe(dead.pipeline.id + 1);
    expect(next.canvas).not.toBe(dead.canvas);
    expect(pipelines).toHaveLength(2);
    expect(canvases).toHaveLength(2);
  });

  it("keeps the healthy session after a broken one was replaced", () => {
    const pipelines: FakePipeline[] = [];
    const canvases: HTMLCanvasElement[] = [];
    const session = makeSession(pipelines, canvases);

    session.acquire().pipeline.broken = true;
    const replacement = session.acquire();
    replacement.pipeline.broken = true;
    const third = session.acquire();

    expect(third.pipeline.id).toBe(3);
    expect(pipelines).toHaveLength(3);
  });

  it("invalidate() forces a fresh session on the next acquire", () => {
    const pipelines: FakePipeline[] = [];
    const canvases: HTMLCanvasElement[] = [];
    const session = makeSession(pipelines, canvases);

    const dead = session.acquire();
    session.invalidate();
    const next = session.acquire();

    expect(next).not.toBe(dead);
    expect(next.canvas).not.toBe(dead.canvas);
    expect(pipelines).toHaveLength(2);
  });

  it("stays empty when pipeline creation throws and retries on next acquire", () => {
    const canvases: HTMLCanvasElement[] = [];
    let attempts = 0;
    const session = new PersistentGlSession<FakePipeline>(
      () => {
        attempts += 1;
        if (attempts === 1) throw new Error("no webgl2");
        return { id: attempts, broken: false, canvas: null };
      },
      () => false,
      () => {
        const canvas = makeCanvas(`c${canvases.length + 1}`);
        canvases.push(canvas);
        return canvas;
      },
    );

    expect(() => session.acquire()).toThrow("no webgl2");
    expect(session.alive).toBe(false);

    const retry = session.acquire();
    expect(retry.pipeline.id).toBe(2);
    expect(session.alive).toBe(true);
  });
});
