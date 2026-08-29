import { PersistentGlSession } from "../gl/persistentGlSession";
import { YaniNekoPipeline } from "./pipeline";
import { yaniQuality } from "./quality";

// Полноэкранное поле Yani Neko живёт на ОДНОМ canvas+контексте всю сессию
// страницы: создание/потеря контекста на каждое переключение темы гонит
// невозвратную ретенцию renderer/GPU в WebView2 (см. gl/persistentGlSession).
// Качество при маунте выставляет поле через setQuality; фидбек-дым сбрасывает
// YaniNekoPipeline.resetFeedback().
export const yaniFieldSession = new PersistentGlSession<YaniNekoPipeline>(
  (canvas) => new YaniNekoPipeline(canvas, yaniQuality("high")),
  (pipeline) => !pipeline.isAlive(),
);
