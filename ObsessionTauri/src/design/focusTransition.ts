export const FOCUS_DURATION = 560;
export const FOCUS_EXIT = 200;
export const FOCUS_ENTER = 360;
export const FOCUS_OVERLAP = 80;
export const FOCUS_EASE = "cubic-bezier(.25,.72,.25,1)";
export const FOCUS_EXIT_EASE = "cubic-bezier(.4,0,.7,1)";

// Only actual glass surfaces and glass-free groups fade. A route-wide opacity
// or filter would change Chromium's backdrop root and flatten the theme.
export function focusTargets(root: HTMLElement): HTMLElement[] {
  const targets: HTMLElement[] = [];
  const visit = (element: HTMLElement) => {
    if (element.hidden || element.matches('[aria-hidden="true"], [data-focus-reflection]')) return;
    if (element.matches(".glass") || !element.querySelector(".glass")) targets.push(element);
    else Array.from(element.children).forEach((child) => visit(child as HTMLElement));
  };
  Array.from(root.children).forEach((child) => visit(child as HTMLElement));
  return targets;
}

type Paint = Partial<Record<"opacity" | "transform" | "filter" | "clipPath", string>>;
type Track = { element: HTMLElement; animation: Animation; properties: (keyof Paint)[] };
type Frozen = { element: HTMLElement; original: Paint; paint: Paint };

// A shared clock overlaps surfaces but gives titles a clean baseline handoff.
// Routes and backend actions commit independently of animation completion.
export class FocusTransition {
  private tracks: Track[] = [];
  private frozen: Frozen[] = [];
  private operation = 0;
  private departing: HTMLElement | null = null;

  stop() {
    ++this.operation;
    // Hide before releasing fill, including exit-only service bodies.
    if (this.departing) this.departing.style.visibility = "hidden";
    this.departing = null;
    this.tracks.splice(0).forEach(({ animation }) => animation.cancel());
    this.frozen.splice(0).forEach(({ element, original }) => Object.assign(element.style, original));
  }

  interrupt(root: HTMLElement) {
    const snapshots: Frozen[] = [];
    if (typeof getComputedStyle === "function") {
      for (const { element, properties } of this.tracks) {
        if (!root.contains(element) || element.matches("[data-focus-reflection]")) continue;
        const computed = getComputedStyle(element);
        const previous = this.frozen.find((item) => item.element === element);
        const original: Paint = {}, paint: Paint = {};
        for (const property of properties) {
          original[property] = previous?.original[property] ?? element.style[property];
          paint[property] = computed[property];
        }
        snapshots.push({ element, original, paint });
      }
    }
    this.stop();
    this.frozen = snapshots;
    snapshots.forEach(({ element, paint }) => Object.assign(element.style, paint));
  }

  private begin(departing: HTMLElement | null = null) {
    // Keep a prepare() snapshot until the replacement exit owns the same frame.
    const frozen = this.frozen;
    this.frozen = [];
    this.stop();
    this.frozen = frozen;
    this.departing = departing;
    if (departing) departing.style.visibility = "";
  }

  private animate(element: HTMLElement, frames: Keyframe[], duration: number, delay = 0, easing = FOCUS_EASE) {
    if (typeof element.animate !== "function") return;
    const animation = element.animate(frames, { duration, delay, easing, fill: "both" });
    const properties = [...new Set(frames.flatMap((frame) => Object.keys(frame)))]
      .filter((property): property is keyof Paint => ["opacity", "transform", "filter", "clipPath"].includes(property));
    this.tracks.push({ element, animation, properties });
    void animation.finished.catch(() => {});
  }

  private passage(root: HTMLElement, entering: boolean, duration: number, delay = 0, stagger = 0, resume = false) {
    focusTargets(root).forEach((target, index) => {
      const glass = target.matches(".glass");
      const title = target.matches("h1") ? target : target.querySelector<HTMLElement>("h1");
      const frozen = this.frozen.find((item) => item.element === target)?.paint;
      const resting = { opacity: 1, transform: "translate3d(0, 0, 0)" };
      const shifted = { opacity: 0, transform: `translate3d(0, ${entering ? 14 : -8}px, 0)` };
      const frames: Keyframe[] = entering ? [resume && frozen ? frozen : shifted, resting] : [{ ...resting, ...frozen }, shifted];
      if (!glass) {
        frames[0].filter = entering ? resume && frozen ? frozen.filter : "blur(2px)" : frozen?.filter ?? "blur(0px)";
        frames[1].filter = entering ? "blur(0px)" : "blur(1.5px)";
      }
      const offset = entering && title ? FOCUS_EXIT : delay + Math.min(index * stagger, FOCUS_OVERLAP);
      this.animate(target, frames, duration, offset, entering ? FOCUS_EASE : FOCUS_EXIT_EASE);
      if (title) {
        const mask = this.frozen.find((item) => item.element === title)?.paint;
        this.animate(title, entering ? [
          { clipPath: "inset(100% 0 -8px 0)" }, { clipPath: "inset(-8px 0 -8px 0)" },
        ] : [
          { clipPath: mask?.clipPath ?? "inset(-8px 0 -8px 0)" }, { clipPath: "inset(-8px 0 100% 0)" },
        ], duration, offset, entering ? FOCUS_EASE : FOCUS_EXIT_EASE);
      }
    });
  }

  private complete() {
    const operation = this.operation;
    void Promise.all(this.tracks.map(({ animation }) => animation.finished)).then(() => {
      if (operation === this.operation) this.stop();
    }).catch(() => {});
  }

  reflect(root: HTMLElement, delay = FOCUS_EXIT - FOCUS_OVERLAP) {
    const lights = Array.from(root.querySelectorAll<HTMLElement>("[data-focus-reflection]"))
      .filter((light) => !light.closest(".glass")?.parentElement?.closest(".glass"));
    lights.slice(0, 2).forEach((light, index) => this.animate(light, [
      { opacity: 0, transform: "translateX(0) rotate(-18deg)" },
      { opacity: .12, offset: .4 },
      { opacity: 0, transform: "translateX(285%) rotate(-18deg)" },
    ], FOCUS_ENTER, delay + index * 32));
  }

  start(departing: HTMLElement | null, arriving: HTMLElement, enabled = true) {
    this.begin(departing);
    if (!enabled) { this.stop(); return; }
    if (departing) this.passage(departing, false, FOCUS_EXIT);
    this.passage(arriving, true, FOCUS_ENTER, FOCUS_EXIT - FOCUS_OVERLAP, 24);
    this.reflect(arriving);
    this.complete();
  }

  reveal(root: HTMLElement, duration = FOCUS_ENTER, resume = false, reflection = true) {
    this.begin();
    this.passage(root, true, duration, 0, 0, resume);
    if (reflection) this.reflect(root.closest<HTMLElement>(".service-workspace") ?? root, 0);
    this.complete();
  }

  exit(root: HTMLElement) {
    this.begin(root);
    this.passage(root, false, FOCUS_EXIT);
    this.complete();
  }
}
