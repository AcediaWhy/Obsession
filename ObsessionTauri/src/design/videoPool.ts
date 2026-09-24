import { restoreMediaSource, releaseMediaSource, type MediaSourceTarget } from "./mediaSource";

/**
 * Пул повторно использует один элемент `<video>` для каждого источника.
 * Это ограничивает число активных медиаконвейеров при смене тем.
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
  // restoreMediaSource не вызывает load() повторно для того же источника.
  restoreMediaSource(video, src);
  pool.set(src, video);
  return video;
}

/** Сколько конвейеров держит пул. Ожидание — не больше числа видео-тем. */
export function videoPoolSize(): number {
  return pool.size;
}

/**
 * Полный сброс пула: releaseMediaSource + снять элементы. Основной вызов —
 * трей-выгрузка (gl/trayStageRelease): конвейеры Catnap/Midnight (~15 МБ за
 * декодер) не нужны, пока окно скрыто; возврат в тему пересоздаст элемент и
 * подгрузит src заново. Тесты используют его же как изоляцию состояния.
 */
export function resetVideoPool(): void {
  pool.forEach((video) => {
    releaseMediaSource(video);
    video.remove();
  });
  pool.clear();
}
