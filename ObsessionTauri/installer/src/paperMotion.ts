const EASE = "cubic-bezier(.22, 1, .36, 1)";
const DURATION = 460;

/** Переходы не ожидаются логикой установки и не задерживают события службы. */
export function createPaperMotion(root: HTMLElement, stage: HTMLElement, artwork: string) {
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  const cat = document.createElement("div");
  cat.className = "scene-cat";
  cat.setAttribute("aria-hidden", "true");
  cat.hidden = true;
  const image = document.createElement("img");
  image.src = artwork;
  image.alt = "";
  image.draggable = false;
  cat.append(image);
  root.append(cat);

  const animations = new Set<Animation>();
  let previousStep = "";
  let previousIndex = 0;
  let oldCat: DOMRect | undefined;
  let oldHeading: DOMRect | undefined;
  let oldAction: DOMRect | undefined;
  let ghost: HTMLElement | undefined;
  // Раскрытие пути и перенос текста меняют высоту экрана без resize окна.
  const layoutObserver = new ResizeObserver(() => alignCat());

  function cancel() {
    for (const animation of animations) animation.cancel();
    animations.clear();
    ghost?.remove();
    ghost = undefined;
  }

  function animate(node: HTMLElement, frames: Keyframe[], duration = DURATION, delay = 0) {
    const animation = node.animate(frames, { duration, delay, easing: EASE, fill: "backwards" });
    animations.add(animation);
    // cancel() отклоняет finished; обработчик нужен и для прерванного перехода.
    void animation.finished.then(
      () => animations.delete(animation),
      () => animations.delete(animation),
    );
    return animation;
  }

  function alignCat() {
    const slot = stage.querySelector<HTMLElement>(".cat-slot");
    cat.hidden = !slot;
    if (!slot) return undefined;
    const rect = slot.getBoundingClientRect();
    const origin = root.getBoundingClientRect();
    Object.assign(cat.style, {
      left: `${rect.left - origin.left - root.clientLeft}px`,
      top: `${rect.top - origin.top - root.clientTop}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
    });
    return rect;
  }

  function capture() {
    // Сохраняем видимое положение до отмены, чтобы новый клик продолжил движение.
    oldCat = cat.hidden ? undefined : cat.getBoundingClientRect();
    oldHeading = stage.querySelector("[data-screen-title]")?.getBoundingClientRect();
    oldAction = root.querySelector("#foot-right .btn")?.getBoundingClientRect();
    cancel();
    const screen = stage.querySelector<HTMLElement>(".screen");
    if (!screen || media.matches) return;
    const rect = screen.getBoundingClientRect();
    const origin = stage.getBoundingClientRect();
    ghost = screen.cloneNode(true) as HTMLElement;
    ghost.classList.add("screen-ghost");
    ghost.setAttribute("aria-hidden", "true");
    ghost.inert = true;
    // Копия только рисуется: без повторных ID, фокуса и обработчиков событий.
    ghost.removeAttribute("id");
    ghost.querySelectorAll("[id]").forEach((node) => node.removeAttribute("id"));
    Object.assign(ghost.style, {
      position: "absolute", margin: "0", left: `${rect.left - origin.left}px`,
      top: `${rect.top - origin.top}px`, width: `${rect.width}px`, height: `${rect.height}px`,
    });
  }

  function play(step: string, index: number) {
    const direction = index < previousIndex ? -1 : 1;
    const shouldAnimate = previousStep !== "" && previousStep !== "bootstrapping"
      && previousStep !== step && step !== "error" && step !== "bootstrapping"
      && !media.matches;
    previousStep = step;
    previousIndex = index;
    layoutObserver.disconnect();
    const currentScreen = stage.querySelector<HTMLElement>(".screen");
    if (currentScreen) layoutObserver.observe(currentScreen);
    const newCat = alignCat();
    if (!shouldAnimate) {
      cancel();
      return;
    }

    if (ghost) {
      const outgoing = ghost;
      stage.append(outgoing);
      const exit = animate(outgoing, [
        { opacity: 1, transform: "translateY(0)" },
        { opacity: 0, transform: `translateY(${-12 * direction}px)` },
      ], 180);
      void exit.finished.then(() => {
        outgoing.remove();
        if (ghost === outgoing) ghost = undefined;
      }, () => outgoing.remove());
    }

    if (newCat && oldCat && newCat.width > 0) {
      animate(cat, [
        { transform: `translate(${oldCat.left - newCat.left}px, ${oldCat.top - newCat.top}px) scale(${oldCat.width / newCat.width})` },
        { transform: "translate(0, 0) scale(1)" },
      ]);
    } else if (newCat) {
      animate(cat, [{ opacity: 0, transform: "translateY(12px)" }, { opacity: 1, transform: "none" }]);
    }

    const screen = stage.querySelector<HTMLElement>(".screen:not(.screen-ghost)");
    const heading = screen?.querySelector<HTMLElement>("[data-screen-title]");
    if (heading) {
      const rect = heading.getBoundingClientRect();
      const dx = oldHeading ? oldHeading.left - rect.left : 0;
      const dy = oldHeading ? oldHeading.top - rect.top : 14 * direction;
      animate(heading, [
        { opacity: 0, transform: `translate(${dx}px, ${dy}px)` },
        { opacity: 1, transform: "translate(0, 0)" },
      ], 420, 35);
    }
    screen?.querySelectorAll<HTMLElement>(
      ".page-caption, .hero-description, .screen-description, .meta-row, .choice-card, .install-location, .quiet-note, .progress-card, .progress-percent, .do-not-close, .result-path, .result-meta, .cat-hello",
    ).forEach((node, index) => {
      // Сохраняем небольшой наклон бумажных карточек из CSS.
      const transform = getComputedStyle(node).transform;
      animate(node, [
        { opacity: 0, transform: `translateY(${18 * direction}px) ${transform === "none" ? "" : transform}` },
        { opacity: 1, transform },
      ], 330, Math.min(65 + index * 24, 155));
    });

    const action = root.querySelector<HTMLElement>("#foot-right .btn");
    if (action) {
      const rect = action.getBoundingClientRect();
      animate(action, [
        { opacity: 0, transform: `translate(${oldAction ? oldAction.left - rect.left : 0}px, 8px)` },
        { opacity: 1, transform: "translate(0, 0)" },
      ], 330, 65);
    }
  }

  const settle = () => { cancel(); alignCat(); };
  media.addEventListener("change", settle);
  window.addEventListener("resize", settle);
  stage.addEventListener("scroll", settle, { passive: true });
  // Загруженный шрифт может изменить положение заголовка и места для котика.
  void document.fonts.ready.then(settle);
  return { capture, play };
}
