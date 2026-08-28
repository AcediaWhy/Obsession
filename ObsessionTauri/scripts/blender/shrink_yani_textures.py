"""Re-exports the shipped Yani GLB with 1024² textures instead of 2048².

Персонаж на экране высотой ~500 px, а base color и normal лежали в 2048²: с
мипами это ~48 МБ видеопамяти против ~13 МБ на 1024². Разницы на глаз нет,
экономия — самая крупная по памяти во всей теме.

Скрипт НЕ сохраняет .blend: мастера в assets-src остаются в 2048², уменьшение
живёт только в экспортируемом GLB. Флаги экспорта переиспользуются из
promote_yani_hybrid, чтобы не разойтись по export_frame_step/оптимизации
анимаций — иначе поедут клипы.

    blender -b -P scripts/blender/shrink_yani_textures.py

GLB под гитом: откат — git checkout -- public/yani/yani-character.glb
"""

from __future__ import annotations

import json
import struct
import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))

from promote_yani_hybrid import MAIN_BLEND, MAIN_GLB, export_glb, find_export_objects

TARGET_SIZE = 1024
SHRINK_IMAGES = ("YaniHybridColor", "normal")


def shrink_images() -> list[str]:
    changed: list[str] = []
    for name in SHRINK_IMAGES:
        image = bpy.data.images.get(name)
        if image is None:
            raise RuntimeError(f"Image '{name}' is missing from {MAIN_BLEND}")
        if max(image.size) <= TARGET_SIZE:
            changed.append(f"{name}=already {image.size[0]}x{image.size[1]}")
            continue
        was = f"{image.size[0]}x{image.size[1]}"
        image.scale(TARGET_SIZE, TARGET_SIZE)
        # Экспортёр glTF умеет отдавать исходные байты файла вместо перекодировки,
        # если картинка «не менялась». Перепаковка кладёт в packed_file уже
        # уменьшенный буфер, поэтому сквозной путь тоже отдаст 1024².
        try:
            image.pack()
        except RuntimeError as error:
            print(f"YANI_PACK_SKIPPED={name}: {error}")
        changed.append(f"{name}={was}->{image.size[0]}x{image.size[1]}")
    return changed


def glb_json(blob: bytes) -> dict:
    json_length = struct.unpack_from("<I", blob, 12)[0]
    return json.loads(blob[20:20 + json_length].decode("utf-8"))


def glb_image_sizes(blob: bytes) -> list[str]:
    """Размеры картинок прямо из записанного GLB — проверка, а не надежда."""
    gltf = glb_json(blob)
    json_length = struct.unpack_from("<I", blob, 12)[0]
    bin_start = 20 + json_length + 8
    sizes: list[str] = []
    for image in gltf.get("images", []):
        view = gltf["bufferViews"][image["bufferView"]]
        start = bin_start + view.get("byteOffset", 0)
        chunk = blob[start:start + view["byteLength"]]
        fourcc = chunk[12:16]
        if fourcc == b"VP8X":
            width = 1 + int.from_bytes(chunk[24:27], "little")
            height = 1 + int.from_bytes(chunk[27:30], "little")
        elif fourcc == b"VP8 ":
            width = struct.unpack_from("<H", chunk, 26)[0] & 0x3FFF
            height = struct.unpack_from("<H", chunk, 28)[0] & 0x3FFF
        elif fourcc == b"VP8L":
            bits = struct.unpack_from("<I", chunk, 21)[0]
            width = (bits & 0x3FFF) + 1
            height = ((bits >> 14) & 0x3FFF) + 1
        else:
            width = height = -1
        sizes.append(f"{image.get('name', '?')}={width}x{height}")
    return sizes


def main() -> None:
    if not MAIN_BLEND.exists():
        raise FileNotFoundError(MAIN_BLEND)
    bpy.ops.wm.open_mainfile(filepath=str(MAIN_BLEND))
    model, armature, shadow = find_export_objects()
    print("YANI_SHRUNK=" + ",".join(shrink_images()))
    export_glb(model, armature, shadow)

    blob = MAIN_GLB.read_bytes()
    gltf = glb_json(blob)
    triangles = sum(
        gltf["accessors"][primitive["indices"]]["count"] // 3
        for mesh in gltf["meshes"]
        for primitive in mesh["primitives"]
        if "indices" in primitive
    )
    print(f"YANI_GLB={MAIN_GLB}")
    print(f"YANI_GLB_BYTES={MAIN_GLB.stat().st_size}")
    print("YANI_GLB_IMAGES=" + ",".join(glb_image_sizes(blob)))
    print(f"YANI_GLB_TRIANGLES={triangles}")
    print("YANI_GLB_CLIPS=" + ",".join(clip["name"] for clip in gltf.get("animations", [])))


if __name__ == "__main__":
    main()
