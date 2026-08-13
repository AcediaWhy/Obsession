export interface MediaSourceTarget {
  getAttribute(name: string): string | null;
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
  load(): void;
  pause(): void;
}

export function restoreMediaSource(media: MediaSourceTarget, src: string) {
  if (media.getAttribute("src") === src) return;

  media.setAttribute("src", src);
  media.load();
}

export function releaseMediaSource(media: MediaSourceTarget) {
  media.pause();
  media.removeAttribute("src");
  media.load();
}
