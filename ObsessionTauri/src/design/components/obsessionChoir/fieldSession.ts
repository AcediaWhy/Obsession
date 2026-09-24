import { PersistentGlSession } from "../gl/persistentGlSession";
import { ObsessionChoirPipeline } from "./pipeline";
import { obsessionChoirQuality } from "./quality";

// Один canvas и GL-контекст сохраняются между переключениями темы.
// World-проход полностью перезаписывает буфер, поэтому сброс при новом
// монтировании не нужен. Качество задаётся через setQuality.
export const choirFieldSession = new PersistentGlSession<ObsessionChoirPipeline>(
  (canvas) => new ObsessionChoirPipeline(canvas, obsessionChoirQuality("high")),
  (pipeline) => !pipeline.isAlive(),
);
