# Rain Warm Lanterns and Storm Implementation Plan

> **SUPERSEDED** (2026-07-18): план прежней «сакура/фонари»-версии темы; реализация заменена сценой «окно ночного поезда» (см. night-train-realism spec).

**Design:** `docs/superpowers/specs/2026-07-18-rain-warm-lanterns-storm-design.md`

**Goal:** тёплый янтарный свет фонарей на холодной ночи (сплит-тон в шейдере)
и драматичная, плавно нарастающая «гроза» при активном обходе.

## Task 1: Split-tone grade в water.frag

**File:** `ObsessionTauri/src/design/components/rain/shaders.ts`

1. Добавить uniform `u_warmth` (float, диапазон 0..1.2, дефолт 1.0).
2. Добавить функцию `warmGrade(vec3 color)`: luminance → `smoothstep` маска
   светов → lerp к янтарю `vec3(1.0, 0.72, 0.42)`, тени сохраняют холодный
   сдвиг; сила эффекта масштабируется `u_warmth`.
3. Применить `warmGrade` к сэмплу `bg` и к `tex` (до blend с shine), чтобы
   капли-линзы показывали тот же грейд.
4. Чистый ALU, без новых текстур и ветвлений по uniform.

## Task 2: Uniform в RainRenderer

**File:** `ObsessionTauri/src/design/components/rain/rainRenderer.ts`

1. Поле `warmth = 1.0` на классе (публичное, пишется из сцены).
2. В `init()` зарегистрировать `u_warmth` со значением по умолчанию.
3. В `draw()` обновлять uniform из `this.warmth` вместе с `u_parallax` —
   безопасный дефолт, если сцена не пишет значение.

## Task 3: Storm factor в RainScene3D

**File:** `ObsessionTauri/src/design/components/RainScene3D.tsx`

1. `stormRef` 0..1, экспоненциальный подход к цели (активен=1/idle=0) с
   постоянной ≈1.5 s; seed от текущего состояния при маунте (горячий старт —
   сразу полная гроза).
2. В фазе `input` derive из `storm`: `rainChance` 0.3→0.7, `rainLimit` 10→26,
   `globalTimeScale` 0.45→0.75, `dropletsRate` 0→30 (округлённые/lerp значения
   присваивать опциям raindrops каждый кадр).
3. В фазе `smooth` считать `warmth = 1.0 + storm * (0.15 + 0.05 * sin(t))` и
   писать в `renderer.warmth`; накапливать `t` по dt.
4. `hotRef` остаётся источником цели storm; paused/reduce-motion не ломают
   переход (invalidate уже есть).

## Task 4: Проверка

1. `npm run build` — TypeScript + Vite production build зелёный.
2. Существующие тесты (`rainFramePipeline.test.ts`, renderer-adjacent) зелёные.
3. Ручной осмотр dev-сцены: фонари янтарные, гроза нарастает ~1.5 s и мягко
   стихает, FPS не просел на high tier.
