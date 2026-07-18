# Night Train Realism (WebGL2) Implementation Plan

**Design:** `docs/superpowers/specs/2026-07-18-night-train-realism-webgl2-design.md`

**Goal:** двухпроходный WebGL2-рендер «ночной поезд» с мип-фокусом,
физикой капель с массой, конденсатом с протиранием и интерьером по фото 4.

## Task 1: WebGL2-каркас и world pass

**Files:** `rain/gl2.ts`, `rain/pipeline.ts`, `rain/worldShaders.ts`,
`rain/quality.ts`, `RainHybridScene.tsx`, `rain/hybridFramePipeline.ts`

1. `gl2.ts`: контекст `webgl2` (null → фолбэк), createProgram с логом ошибок,
   FBO RGBA8 + `generateMipmap`, fullscreen-quad, helpers uniform.
2. `worldShaders.ts`: перенос процедурного мира из `hybridShaders.ts`
   (небо/лес/поле/стволы/морось/зарницы/скоростной смаз) без воды и
   интерьера; ночная палитра сохраняется.
3. `pipeline.ts`: pass A в FBO + мипы; pass B пока «сквозной» (сэмплит мир
   LOD 0 в SDF окна, простая тёмная кайма вне окна).
4. `RainHybridScene`: фазы input→weather→simulate→upload→worldPass→
   compositePass; водная карта временно из текущей `simulation.ts`.
5. `quality.ts`: профиль расширяется (worldScale, mipDepth, dropCap,
   mistGrid); проверка тира обновляет FBO.
6. Проверка: сборка, тесты, скриншот харнесса — мир виден через окно.

## Task 2: dropSim v2 и фокус-LOD

**Files:** `rain/dropSim.ts` (+`dropSim.test.ts`), `rain/waterMap.ts`,
`rain/compositeShaders.ts`, `rain/pipeline.ts`

1. dropSim: масса/состояния (стоит/ползёт, motionInterval), гравитация,
   ветер от weather, слияния с сохранением площади, испарение.
2. Trail deposit: скользящая капля оставляет статичные мелкие (0.3–0.5×),
   худея на trailDropDensity; статичные испаряются.
3. `waterMap.ts`: штампы нормаль/толщина/альфа с альфой по массе (из
   `simulation.ts`, без модели).
4. compositeShaders: `focus = mix(maxLod, minLod, dropMask)`,
   `textureLod(world, uv + normal*refract, focus)`; капли ловят небо
   (ярче стекла) + specular; rim по краю капли.
5. Тесты dropSim: масса (merge+deposit−evaporation), депозит худеет,
   испарение чистит, cap trim; порядок фаз пайплайна.
6. Проверка: тесты, сборка, скриншот `?active=1&warp=1` — капли резкие,
   стекло мутное.

## Task 3: Конденсат (mistSim)

**Files:** `rain/mistSim.ts` (+`mistSim.test.ts`), `rain/compositeShaders.ts`,
`rain/pipeline.ts`, `RainHybridScene.tsx`

1. mistSim: сетка плотности над окном; рост к cap за mistTime; вычитание по
   следам капель (радиус ∝ r капли); медленное восстановление.
2. mist map (R8) аплоадится каждый кадр; шторм ускоряет рост и протирание.
3. Композит: mist поднимает LOD + молочная вуаль; процедурные микрокапли
   (static field) гейтуются плотностью.
4. Тесты: рост/протирание/восстановление; кап в плотности.
5. Проверка: скриншоты idle (муть растёт) и storm (протёртые дорожки).

## Task 4: Интерьер по фото 4 и спрайт-атлас

**Files:** `rain/compositeShaders.ts`, `rain/interiorAtlas.ts`,
`rain/pipeline.ts`

1. Композиция: окно слева (~62% ширины), скругление, уплотнитель, муть у
   рамы; справа верхняя полка (матрас+валики), внизу спальное место с
   подушкой — SDF-силуэты + шумовые складки.
2. Кромочный свет от окна по SDF-дистанции (холодный ключ) + тёплый слабый
   свет купе; интерьер ~90% силуэт.
3. `interiorAtlas.ts`: Canvas2D-атлас (кружка, бутылка, телефон) рисуется
   кодом при инициализации, аплоадится текстурой; размещение — хардкод
   позиции в композите; столик внизу.
4. Проверка: скриншот — читаются полки, подушка, столик с предметами;
   окно не «виньетка», а часть купе.

## Task 5: Полировка, снос старого стека, доки

**Files:** грейд/зерно в `compositeShaders.ts`; удаление файлов из Scope
спеки; `README.md`; спека warm-lanterns помечается superseded.

1. Грейд (sRGB lift), виньетка, тонкое зерно; тюнинг перф-бюджета по тирам.
2. Удалить старый стек и `/public/rain/*`; grep на dangling imports.
3. `npm run build`, весь vitest, финальные скриншоты idle/storm.
4. Обновить README (архитектура rain) и HANDOFF при необходимости.
