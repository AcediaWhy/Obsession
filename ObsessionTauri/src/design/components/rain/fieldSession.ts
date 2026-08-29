import { PersistentGlSession } from "../gl/persistentGlSession";
import { RainPipeline } from "./pipeline";
import { rainQualityProfile } from "./quality";

// Поле Rain живёт на ОДНОМ canvas+контексте всю сессию страницы: destroy() с
// loseContext на каждом unmount гонит невозвратную ретенцию renderer/GPU в
// WebView2 (см. gl/persistentGlSession). Стартовый размер 2×2 — первый же
// initializeScene в RainHybridScene выставляет реальный размер через resize().
// Карты воды/конденсата и текстуры мира перезаливаются на каждом маунте —
// симуляция пересоздаётся, а контекст остаётся.
export const rainFieldSession = new PersistentGlSession<RainPipeline>(
  (canvas) => new RainPipeline(canvas, 2, 2, rainQualityProfile("high")),
  (pipeline) => !pipeline.isAlive(),
);
