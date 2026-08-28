import { useEffect, useRef, type ReactNode } from "react";
import { onRenderActiveChange, renderActive } from "../render";
import { pooledVideo } from "../videoPool";

// Полноэкранный видео-фон для видео-тем (Catnap, Midnight): <video> лупом на
// весь фон + слот оверлеев (тонировка/виньетка темы) поверх. Пауза — по общему
// гейту видимости из render.ts: он уже объединяет Visibility API, сигнал трея
// из Rust и reduce-motion (системный и из настроек), так что контракт тот же,
// что у canvas-тем — в трее декодер стоит, CPU не жжётся.
//
// Сам элемент живёт в videoPool и НЕ создаётся заново на каждый показ темы:
// медиа-конвейер WebView2 переживает откреплённый <video>, и монтирование
// нового элемента на каждое переключение накапливало по ~0.03 ядра и ~15 МБ,
// которые не отдавались даже в трее. Здесь элемент только переезжает в хост.
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
      // play() возвращает промис и может быть отклонён (гонка с pause,
      // автоплей-политика) — глотаем, следующий sync всё поправит.
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
