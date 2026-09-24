import { PersistentGlSession } from "../gl/persistentGlSession";
import { RainPipeline } from "./pipeline";
import { rainQualityProfile } from "./quality";

// Canvas и GL-контекст сохраняются между переключениями темы. При новом
// монтировании сцена и текстуры загружаются заново; resize() задаёт размер
// вместо начальных 2×2 пикселей.
export const rainFieldSession = new PersistentGlSession<RainPipeline>(
  (canvas) => new RainPipeline(canvas, 2, 2, rainQualityProfile("high")),
  (pipeline) => !pipeline.isAlive(),
);
