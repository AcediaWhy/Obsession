"""Попарные кропы референса и скриншота сцены для сверки зон.

Запуск из папки tools/scene-compare: reference.jpg и scene.png (скриншот лабы
1200x896) лежат рядом. Результат: пары файлов pairs/<zone>_ref.png и
pairs/<zone>_scene.png, одинакового масштаба, для визуального сравнения.
"""

import os

from PIL import Image

ZONES = {
    "left-bottom": (0, 440, 320, 896),
    "doorway": (230, 0, 560, 500),
    "cat": (500, 320, 720, 640),
    "sign": (700, 100, 920, 480),
    "right-house": (760, 330, 1200, 896),
    "sky": (560, 0, 1200, 350),
    "balcony-wall": (200, 470, 800, 896),
}


def main():
    reference = Image.open("reference.jpg").convert("RGB")
    scene = Image.open("scene.png").convert("RGB")
    if scene.size != reference.size:
        scene = scene.resize(reference.size, Image.NEAREST)
    os.makedirs("pairs", exist_ok=True)
    for name, box in ZONES.items():
        reference.crop(box).save(os.path.join("pairs", f"{name}_ref.png"))
        scene.crop(box).save(os.path.join("pairs", f"{name}_scene.png"))
    print("pairs written:", len(ZONES))


if __name__ == "__main__":
    main()
