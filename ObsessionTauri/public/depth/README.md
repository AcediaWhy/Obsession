# Depth-parallax ассеты (тема Russia — гибрид фото + снег)

Тема Russia = живое фото (2.5D depth-parallax: фото + карта глубины, шейдер
смещает пиксели по глубине за курсором) + генеративный снег/грейд поверх.
Целевой вайб — `Downloads/idea/vibe.jpg`: синие сумерки, заснеженная тропа,
ели, деревянный забор, дальний тёплый огонёк, падающий снег, тихая меланхолия.

Нужны два файла в `public/depth/russia/`:

- `photo.jpg` — фон в этом вайбе (AI-генерация, см. ниже).
- `depth.png` — карта глубины из этого фото (Depth Anything V2).

Пока их нет — Russia мягко показывает 2D-сцену `RussiaField` (свои снег/зерно/
виньетка), без битых картинок. Пути заданы в `RussiaHybrid.tsx`.

## Шаг 1. Фото (AI-генерация в стиле vibe.jpg)

Локально (ComfyUI / Automatic1111 / Flux) или хостинг (Replicate/Leonardo).
Промпт-заготовка:

> lonely snowy path at blue twilight, snow-laden pine trees, wooden fence,
> a single distant warm street lamp glowing softly, heavy falling snow, muted
> desaturated cold blue palette, melancholic, moody, dark, film grain,
> amateur phone photo, cinematic winter night, high detail

Negative:

> people, text, watermark, logo, oversaturated, warm daylight, summer, sunny

Разрешение — ландшафт под окно приложения, ~1600×1024. Сохрани как
`public/depth/russia/photo.jpg`.

## Шаг 2. Карта глубины (Depth Anything V2, офлайн)

```bash
pip install transformers torch pillow
```

```python
from transformers import pipeline
from PIL import Image

pipe = pipeline("depth-estimation",
                model="depth-anything/Depth-Anything-V2-Small-hf")
depth = pipe(Image.open("photo.jpg"))["depth"]   # near = светлое
depth.save("depth.png")
```

Положить в `public/depth/russia/depth.png`.

## Тонкости

- Если параллакс едет «не туда» (передний план уезжает как задний) — карта
  инвертирована: `from PIL import ImageOps; ImageOps.invert(depth).save(...)`.
- Сила/фокус параллакса — пропсы `scale`/`focus` в `RussiaHybrid.tsx`.
- Плотность/цвет снега и грейд — в `SnowLayer.tsx` и грейд-слоях `RussiaHybrid.tsx`.
- Большие сдвиги вскрывают растяжение на резких краях глубины (нормально для
  одной карты); лечится сегментацией на слои + инпейнтом — следующий шаг, если
  вайб зайдёт.
- Depth-карта может быть меньше фото по разрешению — шейдер сэмплит линейно.
```
