import { PersistentGlSession } from "../gl/persistentGlSession";
import { ObsessionChoirPipeline } from "./pipeline";
import { obsessionChoirQuality } from "./quality";

// Поле Black Choir живёт на ОДНОМ canvas+контексте всю сессию страницы:
// цикл «создать контекст → loseContext» на каждое переключение темы гонит
// невозвратную ретенцию renderer/GPU в WebView2 (см. gl/persistentGlSession).
// Feedback-буферов нет (world-проход переписывает цель целиком каждый кадр),
// поэтому пер-маунтный сброс состояния пайплайну не нужен; качество поле
// выставляет через setQuality.
export const choirFieldSession = new PersistentGlSession<ObsessionChoirPipeline>(
  (canvas) => new ObsessionChoirPipeline(canvas, obsessionChoirQuality("high")),
  (pipeline) => !pipeline.isAlive(),
);
