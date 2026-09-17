# Котик-алхимик — заготовки слоёв v1

Созданы встроенным imagegen из пользовательского PNG. Это сгенерированные заготовки для ручной очистки, НЕ точная пиксельная нарезка. Исходник и рабочая примерка не изменены.

## Файлы

- 00-original-reference.png — неизменённый исходник для сверки.
- 01-body-base.png — основа без глаз и держащих колбу передних лапок; скрытая грудка и живот восстановлены генератором. Хвост и шляпа пока остаются на основе.
- 02-flask-with-paws.png — колба и две держащие лапки одним слоем. Зелье пока запечено в рисунке, не отдельный слой.
- 03-eyes-open.png — открытые глаза.
- 04-eyes-half.png — полуприкрытые глаза.
- 05-eyes-closed.png — закрытые глаза.

## Очистка

Убрать только розовый фон, сохранить PNG с альфа-каналом. Не обрезать пустые поля, не менять размер холста 1254 × 1254 и не масштабировать детали. Белые участки глаз и блики на стекле — часть рисунка, не фон. Проверить края на розовую кайму.

## Ограничения и сборка

Генератор изменил детали и пропорции: особенно открытые/полуприкрытые глаза стали крупнее и сместились. Одинаковый холст НЕ гарантирует точного совмещения. После очистки необходимо выровнять отдельные глаза по размеру и позиции относительно исходника, проверить стыки лапок с туловищем и восстановленные области основы. Готовность к анимации пока не подтверждена. Не заменять этими черновиками рабочую тему автоматически.

## Промпты

Режим: встроенный imagegen, не CLI. Во всех запросах edit target — исходный пользовательский PNG.

### 01-body-base

```text
Use case: precise-object-edit. Input image 1 is the EDIT TARGET, not just a style reference. Prepare one layer of THIS EXACT existing pixel-art cat for manual cleanup and animation. This is a registered sprite layer, not a new illustration. Keep original 1254x1254 square canvas, original pixel scale and original positions. Do not recenter, enlarge, shrink, rotate or restyle any retained part. Preserve stair-step outlines, colors and details. All removed/exterior regions must be one flat opaque chroma-key magenta #FF00FF background for the user to remove manually, with no checkerboard, shadows, haze, labels or text. Retain the cat's hat, ears, head silhouette, nose, torso silhouette, tail and the two bottom hind feet exactly as in the input. Remove both eyes completely (black outlines, white eyeballs and pupils) and inpaint those two areas as plain matching cream facial fur, without eye sockets or marks. Remove the entire green flask AND ONLY the two front paws holding it; reconstruct a plain continuous cream chest/belly behind them, with minimal matching taupe shading. Do not replace the removed holding paws with new paws or arms. Exactly two feet remain at bottom. This is intentionally an eyeless body-base layer awaiting overlay layers. Keep head, hat, nose and bottom feet in the original locations.
```

### 02-flask-with-paws

```text
Use case: precise-object-edit. Input image 1 is the EDIT TARGET, not just a style reference. Prepare one layer of THIS EXACT existing pixel-art cat for manual cleanup and animation. This is a registered sprite layer, not a new illustration. Keep original 1254x1254 square canvas, original pixel scale and original positions. Do not recenter, enlarge, shrink, rotate or restyle any retained part. Preserve stair-step outlines, colors and details. All removed/exterior regions must be one flat opaque chroma-key magenta #FF00FF background for the user to remove manually, with no checkerboard, shadows, haze, labels or text. Keep ONLY the existing green potion flask plus the TWO cream front paws gripping its left and right sides as a single connected assembly. Retain the full flask from mouth at approximately x565..687,y744..795 through round base at y1100; retain the two gripping forepaws approximately x400..548,y865..1005 and x720..855,y865..1005. Keep original flask highlights and bubbles. Erase ALL other cat parts including head, hat, eyes, tail, torso and both bottom feet, replacing them with flat magenta. Do not invent extra arm lengths or add body patches. The resulting isolated assembly must occupy the same lower-central position and original size on the 1254x1254 canvas, NOT be centered/enlarged to fill the image. Most of the image should be empty solid magenta.
```

### 03-eyes-open

```text
Use case: precise-object-edit. Image 1 is the EDIT TARGET. Extract one registered eye-animation layer from this exact pixel cat. Output same 1254x1254 full square canvas, no crop, no zoom, no recentering. Place just TWO EYES at their original on-face coordinates: left eye overall boundary approximately x391..588,y531..720; right eye x680..877,y536..722. Leave original spacing. Everything else must be removed and replaced with flat solid opaque magenta #FF00FF, no checkerboard, no labels, no shadows. Preserve the original thick black chunky stair-step pixel outlines, cream/white/black palette and pixel scale. No head, no nose, no hat, no body, no face patch, no extra decorations. The eyes remain in the middle-height portion of a mostly empty magenta canvas, NOT enlarged to fill the canvas. Keep the two OPEN eyes exactly as in the supplied image: large round white eyes with tall oval black pupils and tiny square white catchlights. Preserve original existing eye shapes and pupil positions. Only remove the rest of the picture.
```

### 04-eyes-half

```text
Use case: precise-object-edit. Image 1 is the EDIT TARGET. Extract one registered eye-animation layer from this exact pixel cat. Output same 1254x1254 full square canvas, no crop, no zoom, no recentering. Place just TWO EYES at their original on-face coordinates: left eye overall boundary approximately x391..588,y531..720; right eye x680..877,y536..722. Leave original spacing. Everything else must be removed and replaced with flat solid opaque magenta #FF00FF, no checkerboard, no labels, no shadows. Preserve the original thick black chunky stair-step pixel outlines, cream/white/black palette and pixel scale. No head, no nose, no hat, no body, no face patch, no extra decorations. The eyes remain in the middle-height portion of a mostly empty magenta canvas, NOT enlarged to fill the canvas. Change the two eyes into the halfway-through-blink frame. Preserve their original width and lower-eye boundary, but lower the upper eyelid to about y625 (half the original height) with a chunky black horizontal stair-stepped lid edge. Below this lid retain the lower part of the white eyeball and corresponding lower part of black pupil until the original lower boundary near y720. Above the lowered lid there must be ONLY magenta, NOT a cream rectangle or facial fur patch. Calm neutral blink, not angry slanted eyebrows. Both eyes blink simultaneously. Keep same positions and scale.
```

### 05-eyes-closed

```text
Use case: precise-object-edit. Input image 1 is the EDIT TARGET. Prepare ONLY the two fully CLOSED eyes of this cat as an isolated pixel-animation layer on the original full 1254x1254 canvas. Remove ALL the rest of the cat, hat, nose, flask and body. Flat solid opaque magenta #FF00FF background, no checkerboard, no labels, no face patches. Two calm closed eyelids, each a thick black simple shallow downward U-shaped curve made of chunky square pixels, about 12px stroke. No white eyeballs or pupils remain. Left closed eyelid should extend approximately x399..583 with its ends at y633 and shallow center at y662; right closed eyelid should extend x686..870, ends at y633, center y662. Preserve the original cat's eye spacing and coarse pixel-art style. DO NOT enlarge or recenter the isolated marks; most of the full square canvas is empty magenta. One pair of closed eyes, one image. No extra objects, no lashes, no eyebrows, no realistic shading.
```

