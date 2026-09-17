from __future__ import annotations

from pathlib import Path

import bpy


PROJECT_ROOT = Path(__file__).resolve().parents[2]
LAB_BLEND = PROJECT_ROOT / "assets-src" / "yani" / "yani-character-lab.blend"
MAIN_BLEND = PROJECT_ROOT / "assets-src" / "yani" / "yani-character.blend"
MAIN_PREVIEW = PROJECT_ROOT / "assets-src" / "yani" / "yani-character-preview.png"
MAIN_GLB = PROJECT_ROOT / "public" / "yani" / "yani-character.glb"
LAB_GLB = PROJECT_ROOT / "public" / "yani" / "yani-character-lab.glb"


def find_export_objects() -> tuple[bpy.types.Object, bpy.types.Object, bpy.types.Object]:
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if len(meshes) < 2:
        raise RuntimeError("Expected the character mesh and contact-shadow mesh")
    model = max(meshes, key=lambda obj: len(obj.data.vertices))
    shadow = min(meshes, key=lambda obj: len(obj.data.vertices))
    armature = next((obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE"), None)
    if armature is None:
        raise RuntimeError("Yani armature was not found")
    return model, armature, shadow


def export_glb(model: bpy.types.Object, armature: bpy.types.Object, shadow: bpy.types.Object) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    model.select_set(True)
    armature.select_set(True)
    shadow.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=str(MAIN_GLB),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=False,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_frame_step=2,
        export_optimize_animation_size=True,
        export_skins=True,
        export_image_format="WEBP",
        export_image_quality=82,
        export_image_webp_fallback=False,
        export_cameras=False,
        export_lights=False,
        export_extras=True,
        export_tangents=True,
    )


def main() -> None:
    if not LAB_BLEND.exists():
        raise FileNotFoundError(LAB_BLEND)
    bpy.ops.wm.open_mainfile(filepath=str(LAB_BLEND))
    model, armature, shadow = find_export_objects()

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 800
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(MAIN_PREVIEW)
    bpy.ops.render.render(write_still=True)

    bpy.ops.file.pack_all()
    bpy.ops.wm.save_as_mainfile(filepath=str(MAIN_BLEND))
    export_glb(model, armature, shadow)
    if LAB_GLB.exists():
        resolved_lab_glb = LAB_GLB.resolve()
        if resolved_lab_glb.parent != MAIN_GLB.resolve().parent:
            raise RuntimeError(f"Refusing to remove unexpected lab asset: {resolved_lab_glb}")
        resolved_lab_glb.unlink()

    print(f"YANI_PROMOTED_BLEND={MAIN_BLEND}")
    print(f"YANI_PROMOTED_PREVIEW={MAIN_PREVIEW}")
    print(f"YANI_PROMOTED_GLB={MAIN_GLB}")
    print(f"YANI_PROMOTED_GLB_BYTES={MAIN_GLB.stat().st_size}")
    print(f"YANI_REMOVED_DUPLICATE_LAB_GLB={LAB_GLB}")


if __name__ == "__main__":
    main()
