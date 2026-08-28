import { restoreMediaSource, releaseMediaSource, type MediaSourceTarget } from "./mediaSource";

/**
 * Один `<video>` на источник, переживающий смену темы.
 *
 * Раньше VideoField монтировал новый элемент на каждый показ видео-темы, а на
 * размонтировании отпускал дорожку (pause + снять src + load()). Замер показал,
 * что этого мало: сам медиа-конвейер — аппаратный декодер в GPU-процессе и его
 * потоки в рендерере — переживает откреплённый элемент. Каждый заход на
 * Catnap/Midnight добавлял ~0.03 ядра и ~15 МБ, и они не отдавались даже в
 * трее: 1 заход — 0.34% CPU и 64 МБ, 7 заходов — 1.99% и 153 МБ при 0.04% и
 * 25 МБ у сессии, где видео-тему не открывали ни разу.
 *
 * Пул делает число конвейеров равным числу видео-тем (их две) вместо одного на
 * каждое переключение. Полностью убрать остаток он не может — конвейер не
 * освобождается и после `releaseMediaSource`, — но рост прекращается.
 */
export interface PooledVideo extends MediaSourceTarget {
  poster: string;
  autoplay: boolean;
  loop: boolean;
  muted: boolean;
  playsInline: boolean;
  preload: string;
  className: string;
  remove(): void;
}

const pool = new Map<string, PooledVideo>();

function createVideoElement(): PooledVideo {
  return document.createElement("video");
}

/**
 * Элемент для источника: из пула либо новый, настроенный и загруженный.
 * `create` внедряется тестами — сам модуль не обязан знать про DOM.
 */
export function pooledVideo(
  src: string,
  poster?: string,
  create: () => PooledVideo = createVideoElement,
): PooledVideo {
  const cached = pool.get(src);
  if (cached) return cached;

  const video = create();
  video.autoplay = true;
  video.loop = true;
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  video.className = "absolute inset-0 h-full w-full object-cover";
  if (poster) video.poster = poster;
  // Через restoreMediaSource, а не video.src: там же живёт правило «не звать
  // load() повторно на том же источнике», и второй путь установки src не нужен.
  restoreMediaSource(video, src);
  pool.set(src, video);
  return video;
}

/** Сколько конвейеров держит пул. Ожидание — не больше числа видео-тем. */
export function videoPoolSize(): number {
  return pool.size;
}

/** Полный сброс. Нужен тестам: модульное состояние иначе течёт между кейсами. */
export function resetVideoPool(): void {
  pool.forEach((video) => {
    releaseMediaSource(video);
    video.remove();
  });
  pool.clear();
}
