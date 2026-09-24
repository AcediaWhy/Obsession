import { useEffect, useRef, type ReactNode } from "react";
import { onRenderActiveChange, renderActive } from "../render";
import { pooledVideo } from "../videoPool";

// Видео-фон тем Catnap и Midnight с оверлеями поверх изображения.
// Элемент берётся из videoPool и переносится между экранами без пересоздания.
// Воспроизведение следует общему состоянию отрисовки из render.ts.
export function VideoField({
  src,
  poster,
  children,
  paused = false,
}: {
  src: string;
  poster?: string;
  children?: ReactNode;
  paused?: boolean;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const video = pooledVideo(src, poster) as HTMLVideoElement;
    videoRef.current = video;
    host.appendChild(video);
    const sync = (active: boolean) => {
      // play() может завершиться ошибкой из-за автоплея или гонки с pause().
      // При следующем изменении состояния вызов повторится.
      if (active && !pausedRef.current) video.play().catch(() => {});
      else video.pause();
    };
    sync(renderActive());
    const unsub = onRenderActiveChange(sync);
    return () => {
      unsub();
      // Элемент возвращается в пул, а не уничтожается: заново созданный <video>
      // стоил бы ещё одной дорожки и ещё одного декодера. Открепляем — иначе
      // скрытый слой продолжает держать слой композитора (замерено: постоянно
      // подключённый элемент дороже на старте и роста всё равно не убирает).
      video.pause();
      video.remove();
      videoRef.current = null;
    };
  }, [poster, src]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (paused || !renderActive()) video.pause();
    else video.play().catch(() => {});
  }, [paused]);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Хост пулового элемента: React держит его пустым и не трогает содержимое,
          <video> приезжает сюда из videoPool. Оверлеи темы идут следом, поэтому
          порядок отрисовки тот же, что был при inline-элементе. */}
      <div ref={hostRef} className="absolute inset-0" />
      {children}
    </div>
  );
}
