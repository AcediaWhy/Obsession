import { useEffect, useRef, type ReactNode } from "react";
import { onRenderActiveChange, renderActive } from "../render";

// Полноэкранный видео-фон для видео-тем (Catnap, Midnight): <video> лупом на
// весь фон + слот оверлеев (тонировка/виньетка темы) поверх. Пауза — по общему
// гейту видимости из render.ts: он уже объединяет Visibility API, сигнал трея
// из Rust и reduce-motion (системный и из настроек), так что контракт тот же,
// что у canvas-тем — в трее декодер стоит, CPU не жжётся.
export function VideoField({
  src,
  poster,
  children,
}: {
  src: string;
  poster?: string;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    const video = ref.current;
    if (!video) return;
    const sync = (active: boolean) => {
      // play() возвращает промис и может быть отклонён (гонка с pause,
      // автоплей-политика) — глотаем, следующий sync всё поправит.
      if (active) video.play().catch(() => {});
      else video.pause();
    };
    sync(renderActive());
    const unsub = onRenderActiveChange(sync);
    return () => {
      unsub();
      // Освобождаем видеодекодер: detached <video> с preload="auto" иначе
      // держит декодер+буфер дорожки (WebView2 не чистит без явного сброса),
      // а в трее GC не идёт → на каждой смене видео-темы копится по декодеру
      // (утечка 200-250 МБ). pause + снять src + load() отпускает ресурс.
      video.pause();
      video.removeAttribute("src");
      video.load();
    };
  }, []);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      <video
        ref={ref}
        src={src}
        poster={poster}
        autoPlay
        loop
        muted
        playsInline
        preload="auto"
        className="absolute inset-0 h-full w-full object-cover"
      />
      {children}
    </div>
  );
}
