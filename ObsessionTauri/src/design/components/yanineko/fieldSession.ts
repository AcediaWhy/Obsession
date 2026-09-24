import { PersistentGlSession } from "../gl/persistentGlSession";
import { YaniNekoPipeline } from "./pipeline";
import { yaniQuality } from "./quality";

// Canvas и GL-контекст сохраняются между переключениями темы. При новом
// монтировании поле задаёт качество и сбрасывает накопленный эффект дыма.
export const yaniFieldSession = new PersistentGlSession<YaniNekoPipeline>(
  (canvas) => new YaniNekoPipeline(canvas, yaniQuality("high")),
  (pipeline) => !pipeline.isAlive(),
);
