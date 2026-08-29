// Трей-выгрузка тяжёлых WebGL-стейджей.
//
// Персистентные сессии тем (gl/persistentGlSession, кеш сцен YaniCharacterScene)
// живут всю страницу — это и убило исходную утечку. Плата: ~30–50 МБ держатся
// даже в трее, где они не нужны. Здесь через несколько минут после скрытия
// окна стейджи полностью выпускаются; возврат в тему пересобираёт сцену один
// раз (~0.5–1.5 с на Yani, остальное мгновенно) — поле тем временем показывает
// свой 2D-фолбэк. Таймер стоит на ровно одну выгрузку за скрытие: пересборки
// чаще раза за трей-цикл означали бы возврат цикла «создать→убить», который
// и был исходной болезнью.
import { onRenderActiveChange } from "../render";
import { resetVideoPool } from "../videoPool";
import { choirFieldSession } from "../components/obsessionChoir/fieldSession";
import { rainFieldSession } from "../components/rain/fieldSession";
import { releaseYaniStages } from "../components/YaniCharacterScene";
import { yaniFieldSession } from "../components/yanineko/fieldSession";

const TRAY_RELEASE_DELAY_MS = 3 * 60_000;

let timer: ReturnType<typeof setTimeout> | null = null;

function releaseAll() {
  timer = null;
  yaniFieldSession.release();
  choirFieldSession.release();
  rainFieldSession.release();
  releaseYaniStages();
  // Видео-декодеры Catnap/Midnight (~15 МБ за конвейер) в трее тоже не нужны;
  // возврат в тему пересоздаст элемент и подгрузит src заново.
  resetVideoPool();
}

/** Подписка один раз на старте приложения. Возвращает отписку. */
export function initTrayStageRelease(): () => void {
  return onRenderActiveChange((active) => {
    if (active) {
      if (timer != null) {
        clearTimeout(timer);
        timer = null;
      }
    } else {
      // Окно скрыто (трей/свёрнуто): поля уже размонтированы (useRenderHidden),
      // выпускать безопасно.
      timer ??= setTimeout(releaseAll, TRAY_RELEASE_DELAY_MS);
    }
  });
}
