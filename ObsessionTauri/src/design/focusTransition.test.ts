import { afterEach, describe, expect, it, vi } from "vitest";
import { FOCUS_DURATION, FOCUS_EXIT, FOCUS_OVERLAP, FocusTransition, focusTargets } from "./focusTransition";

class FakeAnimation {
  cancelled = false;
  finished: Promise<void>;
  private resolve!: () => void;
  private reject!: (error: Error) => void;
  constructor(public frames: Keyframe[], public options: KeyframeAnimationOptions, private onCancel?: () => void) {
    this.finished = new Promise((resolve, reject) => { this.resolve = resolve; this.reject = reject; });
  }
  finish() { this.resolve(); }
  cancel() { this.onCancel?.(); this.cancelled = true; this.reject(new Error("cancelled")); }
}

class FakeElement {
  hidden = false;
  parentElement: FakeElement | null = null;
  children: FakeElement[] = [];
  style: Record<string, string> = { opacity: "", transform: "", filter: "", clipPath: "", visibility: "" };
  computed: Record<string, string> = {};
  onCancel?: () => void;
  animations: FakeAnimation[] = [];
  constructor(private selectors: string[] = []) {}
  append(...children: FakeElement[]) {
    children.forEach((child) => { child.parentElement = this; this.children.push(child); });
    return this;
  }
  matches(selector: string) { return selector.split(",").some((part) => this.selectors.includes(part.trim())); }
  querySelector(selector: string): FakeElement | null { return this.querySelectorAll(selector)[0] ?? null; }
  querySelectorAll(selector: string): FakeElement[] {
    return this.children.flatMap((child) => [
      ...(child.matches(selector) ? [child] : []), ...child.querySelectorAll(selector),
    ]);
  }
  closest(selector: string): FakeElement | null { return this.matches(selector) ? this : this.parentElement?.closest(selector) ?? null; }
  contains(element: FakeElement): boolean { return this === element || this.children.some((child) => child.contains(element)); }
  animate(frames: Keyframe[], options: KeyframeAnimationOptions) {
    const animation = new FakeAnimation(frames, options, () => this.onCancel?.());
    this.animations.push(animation);
    return animation;
  }
  allAnimations(): FakeAnimation[] { return [...this.animations, ...this.children.flatMap((child) => child.allAnimations())]; }
}

const el = (value: FakeElement) => value as unknown as HTMLElement;
const settle = async () => { for (let i = 0; i < 5; ++i) await Promise.resolve(); };
const end = (animation: FakeAnimation) => Number(animation.options.delay ?? 0) + Number(animation.options.duration);

afterEach(() => vi.unstubAllGlobals());

function computedStyles() {
  vi.stubGlobal("getComputedStyle", (element: FakeElement) => ({ ...element.style, ...element.computed }));
}

function route() {
  const title = new FakeElement(["h1"]);
  const heading = new FakeElement().append(title);
  const content = new FakeElement();
  const light = new FakeElement(["[data-focus-reflection]"]);
  const glass = new FakeElement([".glass"]).append(content, light);
  const glassAncestor = new FakeElement().append(glass);
  const root = new FakeElement().append(heading, glassAncestor);
  return { root, heading, title, glassAncestor, glass, content, light };
}

describe("focus targets", () => {
  it("fades real surfaces and whole glass-free groups without touching backdrop ancestors", () => {
    const scene = route();
    expect(focusTargets(el(scene.root))).toEqual([scene.heading, scene.glass]);
    const controller = new FocusTransition();
    controller.start(null, el(scene.root));
    expect(scene.root.animations).toHaveLength(0);
    expect(scene.glassAncestor.animations).toHaveLength(0);
    expect(scene.content.animations).toHaveLength(0);
    expect(scene.heading.animations[0].frames[0].transform).toBeDefined();
    expect(scene.glass.animations[0].frames.every((frame) => frame.filter === undefined)).toBe(true);
    controller.stop();
  });

  it("ignores hidden/exiting subtrees and decorative reflections as content", () => {
    const hidden = new FakeElement().append(new FakeElement([".glass"]));
    hidden.hidden = true;
    const exiting = new FakeElement(['[aria-hidden="true"]']).append(new FakeElement([".glass"]));
    const light = new FakeElement(["[data-focus-reflection]"]);
    const content = new FakeElement();
    const root = new FakeElement().append(hidden, exiting, light, content);
    expect(focusTargets(el(root))).toEqual([content]);
  });
});

describe("focus transition lifecycle", () => {
  it("overlaps surfaces on the shared clock while titles wait for their baseline to clear", () => {
    const from = route(), to = route();
    const controller = new FocusTransition();
    controller.start(el(from.root), el(to.root));
    expect(FOCUS_DURATION).toBe(560);
    expect(from.heading.animations[0].options.duration).toBe(FOCUS_EXIT);
    const incomingGlass = to.glass.animations[0];
    expect(Number(incomingGlass.options.delay)).toBeGreaterThanOrEqual(FOCUS_EXIT - FOCUS_OVERLAP);
    expect(Number(incomingGlass.options.delay)).toBeLessThan(end(from.glass.animations[0]));
    expect(Number(to.heading.animations[0].options.delay)).toBeGreaterThanOrEqual(end(from.heading.animations[0]));
    expect(Number(to.title.animations[0].options.delay)).toBeGreaterThanOrEqual(end(from.title.animations[0]));
    expect(Math.max(...to.root.allAnimations().map(end))).toBeLessThanOrEqual(FOCUS_DURATION);
    controller.stop();
  });

  it("cancels all fills on interruption without changing authored incoming paint", async () => {
    const from = route(), to = route();
    to.glass.style.opacity = "0.8";
    to.heading.style.filter = "none";
    const controller = new FocusTransition();
    controller.start(el(from.root), el(to.root));
    const running = [...from.root.allAnimations(), ...to.root.allAnimations()];
    controller.stop();
    await settle();
    expect(running.length).toBeGreaterThan(0);
    expect(running.every((animation) => animation.cancelled)).toBe(true);
    expect(to.glass.style.opacity).toBe("0.8");
    expect(to.heading.style.filter).toBe("none");
    expect(to.root.style.visibility).toBe("");
    expect(from.root.style.visibility).toBe("hidden");
    controller.stop();
    expect(to.root.style.visibility).toBe("");
  });

  it("does not let a previous completion stop a rapid replacement", async () => {
    const first = route(), second = route(), third = route();
    const controller = new FocusTransition();
    controller.start(el(first.root), el(second.root));
    const previous = [...first.root.allAnimations(), ...second.root.allAnimations()];
    previous.forEach((animation) => animation.finish());
    controller.start(el(second.root), el(third.root));
    const replacement = [...second.root.allAnimations(), ...third.root.allAnimations()]
      .filter((animation) => !previous.includes(animation));
    await settle();
    expect(previous.every((animation) => animation.cancelled)).toBe(true);
    expect(replacement.every((animation) => !animation.cancelled)).toBe(true);
    expect(second.root.style.visibility).toBe("");
    expect(third.root.style.visibility).toBe("");
    replacement.forEach((animation) => animation.finish());
    await settle();
    expect(replacement.every((animation) => animation.cancelled)).toBe(true);
    expect(second.root.style.visibility).toBe("hidden");
    expect(third.root.style.visibility).toBe("");
  });

  it("commits the incoming content immediately when motion is disabled", async () => {
    const from = route(), to = route();
    const controller = new FocusTransition();
    controller.start(el(from.root), el(to.root), false);
    await settle();
    expect([...from.root.allAnimations(), ...to.root.allAnimations()]).toHaveLength(0);
    expect(from.root.style.visibility).toBe("hidden");
    expect(to.root.style.visibility).toBe("");
    expect(to.glass.style.opacity).toBe("");
  });

  it("falls back to visible content when WAAPI is unavailable", async () => {
    const from = route(), to = route();
    for (const target of [from.heading, from.title, from.glass, from.light, to.heading, to.title, to.glass, to.light]) {
      Object.defineProperty(target, "animate", { value: undefined });
    }
    const controller = new FocusTransition();
    expect(() => controller.start(el(from.root), el(to.root))).not.toThrow();
    await settle();
    expect([...from.root.allAnimations(), ...to.root.allAnimations()]).toHaveLength(0);
    expect(from.root.style.visibility).toBe("hidden");
    expect(to.root.style.visibility).toBe("");
    expect(to.heading.style.opacity).toBe("");
  });

  it("hides an exit-only body before cancelling its fill so its authored paint cannot flash", async () => {
    const scene = route();
    const observed: string[] = [];
    scene.heading.onCancel = scene.title.onCancel = scene.glass.onCancel = () => observed.push(scene.root.style.visibility);
    const controller = new FocusTransition();
    controller.exit(el(scene.root));
    scene.root.allAnimations().forEach((animation) => animation.finish());
    await settle();
    expect(observed.length).toBeGreaterThan(0);
    expect(observed.every((visibility) => visibility === "hidden")).toBe(true);
    expect(scene.root.style.visibility).toBe("hidden");
  });

  it("continues a replacement exit from frozen computed paint and restores authored styles at rest", async () => {
    computedStyles();
    const first = route(), second = route(), third = route();
    Object.assign(second.heading.style, { opacity: "0.8", transform: "translateY(1px)", filter: "none" });
    second.title.style.clipPath = "none";
    Object.assign(second.heading.computed, { opacity: "0.42", transform: "matrix(1, 0, 0, 1, 0, 9)", filter: "blur(1.2px)" });
    second.title.computed.clipPath = "inset(42% 0px 0px 0px)";
    const authored = { ...second.heading.style };
    const controller = new FocusTransition();
    controller.start(el(first.root), el(second.root));
    const previous = [...first.root.allAnimations(), ...second.root.allAnimations()];
    controller.interrupt(el(second.root));
    expect(previous.every((animation) => animation.cancelled)).toBe(true);
    expect(second.heading.style.opacity).toBe("0.42");
    expect(second.title.style.clipPath).toBe(second.title.computed.clipPath);
    expect(second.light.style.opacity).toBe("");

    controller.start(el(second.root), el(third.root));
    const replacementExit = second.heading.animations[second.heading.animations.length - 1];
    expect(replacementExit.frames[0]).toMatchObject(second.heading.computed);
    expect(second.title.animations[second.title.animations.length - 1].frames[0].clipPath).toBe(second.title.computed.clipPath);
    expect(second.root.style.visibility).toBe("");
    const replacement = [...second.root.allAnimations(), ...third.root.allAnimations()].filter((animation) => !previous.includes(animation));
    replacement.forEach((animation) => animation.finish());
    await settle();
    expect(second.heading.style).toEqual(authored);
    expect(second.title.style.clipPath).toBe("none");
    expect(second.root.style.visibility).toBe("hidden");
    expect(third.root.style.visibility).toBe("");
  });

  it("resumes reveal from the interrupted paint instead of replaying the hidden entrance", async () => {
    computedStyles();
    const scene = route();
    scene.glass.style.opacity = "0.9";
    Object.assign(scene.glass.computed, { opacity: "0.58", transform: "matrix(1, 0, 0, 1, 0, 6)" });
    const controller = new FocusTransition();
    controller.reveal(el(scene.root));
    controller.interrupt(el(scene.root));
    controller.reveal(el(scene.root), undefined, true);
    expect(scene.glass.animations[scene.glass.animations.length - 1].frames[0]).toMatchObject(scene.glass.computed);
    controller.stop();
    await settle();
    expect(scene.glass.style.opacity).toBe("0.9");
    expect(scene.glass.style.transform).toBe("");
    expect(scene.root.style.visibility).toBe("");
  });
});
