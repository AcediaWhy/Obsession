from __future__ import annotations

import math
import sys
from pathlib import Path

import bpy


PROJECT_ROOT = Path(__file__).resolve().parents[2]
CURRENT_BLEND = PROJECT_ROOT / "assets-src" / "yani" / "yani-character.blend"
CURRENT_COLOR = PROJECT_ROOT / "assets-src" / "yani" / "optimized-textures" / "yani-color.webp"
HYBRID_COLOR = PROJECT_ROOT / "assets-src" / "yani" / "optimized-textures" / "yani-hybrid-color.webp"
LAB_BLEND = PROJECT_ROOT / "assets-src" / "yani" / "yani-character-lab.blend"
LAB_PREVIEW = PROJECT_ROOT / "assets-src" / "yani" / "yani-character-lab-preview.png"
LAB_GLB = PROJECT_ROOT / "public" / "yani" / "yani-character-lab.glb"


def smoothstep(edge0: float, edge1: float, value: float) -> float:
    t = max(0.0, min(1.0, (value - edge0) / max(1e-6, edge1 - edge0)))
    return t * t * (3.0 - 2.0 * t)


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)


def import_donor(source: Path) -> bpy.types.Object:
    bpy.ops.import_scene.gltf(filepath=str(source))
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if not meshes:
        raise RuntimeError("Donor GLB contains no mesh")
    return max(meshes, key=lambda obj: len(obj.data.vertices))


def add_tail_mask(model: bpy.types.Object) -> None:
    mesh = model.data
    old = mesh.color_attributes.get("hybrid_tail")
    if old is not None:
        mesh.color_attributes.remove(old)
    attribute = mesh.color_attributes.new(name="hybrid_tail", type="FLOAT_COLOR", domain="POINT")
    for vertex in mesh.vertices:
        point = vertex.co
        x_weight = smoothstep(0.12, 0.43, point.x)
        y_weight = smoothstep(0.04, 0.27, point.y)
        z_weight = 1.0 - smoothstep(0.08, 0.24, point.z)
        weight = x_weight * y_weight * z_weight
        attribute.data[vertex.index].color = (weight, weight, weight, 1.0)


def build_hybrid_material(model: bpy.types.Object, donor_color: bpy.types.Image) -> bpy.types.Image:
    current_color = bpy.data.images.load(str(CURRENT_COLOR), check_existing=False)
    current_color.colorspace_settings.name = "sRGB"
    donor_color.colorspace_settings.name = "sRGB"

    baked = bpy.data.images.new("YaniHybridColor", width=2048, height=2048, alpha=True)
    baked.colorspace_settings.name = "sRGB"

    material = bpy.data.materials.new("YaniHybridBake")
    material.use_nodes = True
    nodes = material.node_tree.nodes
    links = material.node_tree.links
    nodes.clear()

    output = nodes.new("ShaderNodeOutputMaterial")
    emission = nodes.new("ShaderNodeEmission")
    donor_node = nodes.new("ShaderNodeTexImage")
    donor_node.image = donor_color
    donor_node.interpolation = "Linear"
    current_node = nodes.new("ShaderNodeTexImage")
    current_node.image = current_color
    current_node.interpolation = "Linear"

    base_mix = nodes.new("ShaderNodeMixRGB")
    base_mix.blend_type = "MIX"
    base_mix.inputs[0].default_value = 0.30
    links.new(donor_node.outputs["Color"], base_mix.inputs[1])
    links.new(current_node.outputs["Color"], base_mix.inputs[2])

    current_bw = nodes.new("ShaderNodeRGBToBW")
    links.new(current_node.outputs["Color"], current_bw.inputs[0])
    outline_ramp = nodes.new("ShaderNodeValToRGB")
    outline_ramp.color_ramp.interpolation = "EASE"
    outline_ramp.color_ramp.elements[0].position = 0.16
    outline_ramp.color_ramp.elements[0].color = (1.0, 1.0, 1.0, 1.0)
    outline_ramp.color_ramp.elements[1].position = 0.43
    outline_ramp.color_ramp.elements[1].color = (0.0, 0.0, 0.0, 1.0)
    links.new(current_bw.outputs[0], outline_ramp.inputs[0])
    outline_strength = nodes.new("ShaderNodeMath")
    outline_strength.operation = "MULTIPLY"
    outline_strength.inputs[1].default_value = 0.68
    links.new(outline_ramp.outputs[0], outline_strength.inputs[0])

    separate = nodes.new("ShaderNodeSeparateColor")
    separate.mode = "RGB"
    links.new(current_node.outputs["Color"], separate.inputs[0])
    red_minus_green = nodes.new("ShaderNodeMath")
    red_minus_green.operation = "SUBTRACT"
    links.new(separate.outputs["Red"], red_minus_green.inputs[0])
    links.new(separate.outputs["Green"], red_minus_green.inputs[1])
    red_minus_blue = nodes.new("ShaderNodeMath")
    red_minus_blue.operation = "SUBTRACT"
    links.new(separate.outputs["Red"], red_minus_blue.inputs[0])
    links.new(separate.outputs["Blue"], red_minus_blue.inputs[1])
    red_delta = nodes.new("ShaderNodeMath")
    red_delta.operation = "MINIMUM"
    links.new(red_minus_green.outputs[0], red_delta.inputs[0])
    links.new(red_minus_blue.outputs[0], red_delta.inputs[1])
    cheek_mask = nodes.new("ShaderNodeMath")
    cheek_mask.operation = "GREATER_THAN"
    cheek_mask.inputs[1].default_value = 0.18
    links.new(red_delta.outputs[0], cheek_mask.inputs[0])

    feature_mask = nodes.new("ShaderNodeMath")
    feature_mask.operation = "MAXIMUM"
    links.new(outline_strength.outputs[0], feature_mask.inputs[0])
    links.new(cheek_mask.outputs[0], feature_mask.inputs[1])
    feature_mix = nodes.new("ShaderNodeMixRGB")
    links.new(feature_mask.outputs[0], feature_mix.inputs[0])
    links.new(base_mix.outputs[0], feature_mix.inputs[1])
    links.new(current_node.outputs["Color"], feature_mix.inputs[2])

    tail_attribute = nodes.new("ShaderNodeVertexColor")
    tail_attribute.layer_name = "hybrid_tail"
    final_mix = nodes.new("ShaderNodeMixRGB")
    links.new(tail_attribute.outputs["Color"], final_mix.inputs[0])
    links.new(feature_mix.outputs[0], final_mix.inputs[1])
    links.new(current_node.outputs["Color"], final_mix.inputs[2])
    links.new(final_mix.outputs[0], emission.inputs["Color"])
    links.new(emission.outputs[0], output.inputs["Surface"])

    target = nodes.new("ShaderNodeTexImage")
    target.image = baked
    nodes.active = target
    target.select = True

    model.data.materials.clear()
    model.data.materials.append(material)
    return baked


def bake_hybrid(model: bpy.types.Object, baked: bpy.types.Image) -> None:
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 1
    bpy.ops.object.select_all(action="DESELECT")
    model.select_set(True)
    bpy.context.view_layer.objects.active = model
    bpy.ops.object.bake(type="EMIT", margin=10, use_clear=True)
    scene.render.image_settings.file_format = "WEBP"
    scene.render.image_settings.quality = 88
    baked.filepath_raw = str(HYBRID_COLOR)
    baked.file_format = "WEBP"
    baked.save()


def install_hybrid_into_lab_scene() -> tuple[bpy.types.Object, bpy.types.Object, bpy.types.Object]:
    bpy.ops.wm.open_mainfile(filepath=str(CURRENT_BLEND))
    hybrid = bpy.data.images.load(str(HYBRID_COLOR), check_existing=False)
    hybrid.name = "YaniHybridColor"
    hybrid.colorspace_settings.name = "sRGB"
    hybrid.filepath = bpy.path.relpath(str(HYBRID_COLOR))

    replaced = 0
    for material in bpy.data.materials:
        if not material.use_nodes:
            continue
        for node in material.node_tree.nodes:
            if node.type != "TEX_IMAGE" or node.image is None:
                continue
            path = Path(bpy.path.abspath(node.image.filepath)) if node.image.filepath else None
            if node.image.name == "Image_0" or (path and path.name == CURRENT_COLOR.name):
                node.image = hybrid
                replaced += 1
    if replaced == 0:
        raise RuntimeError("Current Yani base-color node was not found")

    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    model = max(meshes, key=lambda obj: len(obj.data.vertices))
    shadow = min(meshes, key=lambda obj: len(obj.data.vertices))
    armature = next(obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE")
    return model, armature, shadow


def save_preview_and_export(model: bpy.types.Object, armature: bpy.types.Object, shadow: bpy.types.Object) -> None:
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 800
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(LAB_PREVIEW)
    bpy.ops.render.render(write_still=True)

    bpy.ops.file.pack_all()
    bpy.ops.wm.save_as_mainfile(filepath=str(LAB_BLEND))

    bpy.ops.object.select_all(action="DESELECT")
    model.select_set(True)
    armature.select_set(True)
    shadow.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=str(LAB_GLB),
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
    args = sys.argv[sys.argv.index("--") + 1:]
    if not args:
        raise RuntimeError("Pass the donor GLB after --")
    donor_path = Path(args[0]).resolve()
    if not donor_path.exists():
        raise FileNotFoundError(donor_path)
    for path in (CURRENT_BLEND, CURRENT_COLOR):
        if not path.exists():
            raise FileNotFoundError(path)

    HYBRID_COLOR.parent.mkdir(parents=True, exist_ok=True)
    LAB_GLB.parent.mkdir(parents=True, exist_ok=True)
    clear_scene()
    donor_model = import_donor(donor_path)
    donor_material = next((slot.material for slot in donor_model.material_slots if slot.material and slot.material.use_nodes), None)
    if donor_material is None:
        raise RuntimeError("Donor material was not found")
    donor_color_node = next(
        (node for node in donor_material.node_tree.nodes if node.type == "TEX_IMAGE" and node.image and node.image.colorspace_settings.name == "sRGB"),
        None,
    )
    if donor_color_node is None:
        raise RuntimeError("Donor base-color image was not found")
    donor_color = donor_color_node.image

    add_tail_mask(donor_model)
    baked = build_hybrid_material(donor_model, donor_color)
    bake_hybrid(donor_model, baked)
    model, armature, shadow = install_hybrid_into_lab_scene()
    save_preview_and_export(model, armature, shadow)

    print(f"YANI_HYBRID_DONOR={donor_path}")
    print(f"YANI_HYBRID_COLOR={HYBRID_COLOR}")
    print(f"YANI_HYBRID_BLEND={LAB_BLEND}")
    print(f"YANI_HYBRID_PREVIEW={LAB_PREVIEW}")
    print(f"YANI_HYBRID_GLB={LAB_GLB}")
    print(f"YANI_HYBRID_GLB_BYTES={LAB_GLB.stat().st_size}")


if __name__ == "__main__":
    main()
