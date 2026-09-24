// После скрытия окна освобождаем WebGL-сессии тем и видео-декодеры.
// Задержка исключает пересборку сцены при кратковременном скрытии; за один
// цикл скрытия ресурсы освобождаются только один раз.
import { onRenderActiveChange, renderHidden } from "../render";
import { resetVideoPool } from "../videoPool";
import { choirFieldSession } from "../components/obsessionChoir/fieldSession";
import { rainFieldSession } from "../components/rain/fieldSession";
import { releaseYaniStages } from "../components/YaniCharacterScene";
import { yaniFieldSession } from "../components/yanineko/fieldSession";

const TRAY_RELEASE_DELAY_MS = 1000;

function releaseAll() {
  yaniFieldSession.release();
  choirFieldSession.release();
  rainFieldSession.release();
  releaseYaniStages();
  // Видео-декодеры пересоздаются при возврате к теме.
  resetVideoPool();
}

/** Подписка один раз на старте приложения. Возвращает отписку. */
export function initTrayStageRelease(): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let released = false;
  const update = () => {
    if (renderHidden()) {
      if (!released && timer === null) timer = setTimeout(() => {
        timer = null;
        if (!renderHidden()) return;
        released = true;
        releaseAll();
      }, TRAY_RELEASE_DELAY_MS);
    } else {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      released = false;
    }
  };
  const unsubscribe = onRenderActiveChange(update);
  update();
  return () => { unsubscribe(); if (timer !== null) clearTimeout(timer); };
}
