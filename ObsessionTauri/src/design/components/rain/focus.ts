// Политика фокуса для композита Rain. Вынесена из пайплайна отдельным чистым
// модулем, чтобы инвариант «внутри капли резче, чем стекло вокруг» проверялся
// юнит-тестом без GL-контекста: именно его нарушение делало капли невидимыми
// (LOD 3.3 внутри против 0 вокруг → замер A/B давал разницу 0.24/255).

export type RainFocusLods = {
  /** Уровень мипа внутри капли: практически резко. */
  dropLod: number;
  /** Базовая расфокусировка стекла вне капель. */
  glassLod: number;
  /** Добавка размытия от конденсата, поверх glassLod. */
  mistLod: number;
  /** Широкая вуаль рассеяния в запотевшем слое. */
  scatterLod: number;
};

/** Привязка к ширине FBO держит одинаковое относительное размытие на любом
 *  разрешении: 460 px — эталонная «ширина кружка нерезкости» кадра. */
export function rainFocusLods(fboWidth: number, mipDepth: number): RainFocusLods {
  const width = Math.max(1, fboWidth);
  const depth = Math.max(0, mipDepth);
  const glassLod = Math.min(depth, Math.max(0.6, Math.log2(width / 460)));
  // Нутро капли примерно на одну ступень резче стекла. Референсы расходятся в
  // деталях (codrops берёт внутрь капли даже более мягкую картинку, но светлее;
  // Heartfelt — чуть резче фона), сходятся в одном: стекло не в фокусе. Вывод
  // из glassLod гарантирует, что капля не окажется мягче стекла.
  const dropLod = Math.min(glassLod, Math.max(0.3, glassLod - 1.1));
  const mistLod = Math.min(Math.max(0, depth - glassLod), 1.5);
  const scatterLod = Math.min(depth, glassLod + 4.0);
  return { dropLod, glassLod, mistLod, scatterLod };
}
