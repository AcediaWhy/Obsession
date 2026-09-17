from __future__ import annotations

import math
from pathlib import Path

import bpy
from mathutils import Vector


PROJECT_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SOURCE = Path(r"C:\Users\Usogui\Downloads\Meshy_AI_Smug_Cat_Nap_0825131012_texture.glb")
OUTPUT_BLEND = PROJECT_ROOT / "assets-src" / "yani" / "yani-character.blend"
OUTPUT_GLB = PROJECT_ROOT / "public" / "yani" / "yani-character.glb"
PREVIEW_PNG = PROJECT_ROOT / "assets-src" / "yani" / "yani-character-preview.png"
TEXTURE_DIR = PROJECT_ROOT / "assets-src" / "yani" / "optimized-textures"

FPS = 24


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (
        bpy.data.meshes,
        bpy.data.armatures,
        bpy.data.cameras,
        bpy.data.lights,
        bpy.data.actions,
    ):
        for datablock in list(datablocks):
            if datablock.users == 0:
                datablocks.remove(datablock)


def import_character(source: Path) -> bpy.types.Object:
    before = set(bpy.context.scene.objects)
    bpy.ops.import_scene.gltf(filepath=str(source))
    imported = [obj for obj in bpy.context.scene.objects if obj not in before]
    meshes = [obj for obj in imported if obj.type == "MESH" and len(obj.data.vertices) > 100]
    if not meshes:
        raise RuntimeError("Meshy character mesh was not found")
    model = max(meshes, key=lambda obj: len(obj.data.vertices))
    for obj in imported:
        if obj is not model:
            bpy.data.objects.remove(obj, do_unlink=True)
    model.name = "YaniCharacter"
    model.data.name = "YaniCharacterMesh"
    model["yani_role"] = "character"
    return model


def optimize_textures() -> None:
    TEXTURE_DIR.mkdir(parents=True, exist_ok=True)
    targets = {
        "Image_0": (2048, "yani-color.webp"),
        "Image_1": (1024, "yani-surface.webp"),
        "normal": (2048, "yani-normal.webp"),
    }
    bpy.context.scene.render.image_settings.quality = 82
    for name, (size, filename) in targets.items():
        image = bpy.data.images.get(name)
        if image is None:
            continue
        color_space = image.colorspace_settings.name
        image.scale(size, size)
        image.filepath_raw = str(TEXTURE_DIR / filename)
        image.file_format = "WEBP"
        image.save()
        if image.packed_file is not None:
            image.unpack(method="REMOVE")
        image.reload()
        image.colorspace_settings.name = color_space


def clamp01(value: float) -> float:
    return max(0.0, min(1.0, value))


def smoothstep(edge0: float, edge1: float, value: float) -> float:
    t = clamp01((value - edge0) / max(1e-6, edge1 - edge0))
    return t * t * (3.0 - 2.0 * t)


def ellipsoid_weight(point: Vector, center: Vector, radii: Vector, inner: float = 0.55) -> float:
    distance = math.sqrt(
        ((point.x - center.x) / radii.x) ** 2
        + ((point.y - center.y) / radii.y) ** 2
        + ((point.z - center.z) / radii.z) ** 2
    )
    return 1.0 - smoothstep(inner, 1.0, distance)


def segment_projection(point: Vector, start: Vector, end: Vector) -> float:
    axis = end - start
    return clamp01((point - start).dot(axis) / max(1e-6, axis.length_squared))


def capsule_weight(point: Vector, start: Vector, end: Vector, radius: float, inner: float = 0.46) -> float:
    axis = end - start
    projection = clamp01((point - start).dot(axis) / max(1e-6, axis.length_squared))
    distance = (point - (start + axis * projection)).length
    return 1.0 - smoothstep(radius * inner, radius, distance)


def reshape_feet(model: bpy.types.Object) -> None:
    """Round the generated sock geometry without adding vertices or triangles."""
    mesh = model.data
    for vertex in mesh.vertices:
        point = vertex.co.copy()

        left_toe = smoothstep(0.50, 0.69, -point.x)
        left_toe *= 1.0 - smoothstep(-0.68, -0.57, point.z)
        left_toe *= 1.0 - smoothstep(0.30, 0.43, abs(point.y + 0.13))
        left_heel = ellipsoid_weight(point, Vector((-0.43, -0.01, -0.82)), Vector((0.24, 0.36, 0.23)), inner=0.28)

        right_toe = smoothstep(0.29, 0.46, -point.y)
        right_toe *= 1.0 - smoothstep(-0.72, -0.60, point.z)
        right_toe *= 1.0 - smoothstep(0.22, 0.34, abs(point.x - 0.04))
        right_heel = ellipsoid_weight(point, Vector((0.08, -0.22, -0.83)), Vector((0.27, 0.25, 0.23)), inner=0.28)

        left_weight = max(left_toe, left_heel)
        right_weight = max(right_toe, right_heel)

        if left_weight > 1e-5:
            point.y += (point.y + 0.13) * (0.13 * left_toe + 0.075 * left_heel)
            point.z += (point.z + 0.82) * (0.075 * left_toe + 0.055 * left_heel)
            point += vertex.normal.normalized() * (0.010 * left_weight)
            if point.x < -0.62:
                radial = max(0.0, 1.0 - ((point.y + 0.13) / 0.29) ** 2 - ((point.z + 0.82) / 0.21) ** 2)
                rounded_x = -0.66 - 0.15 * math.sqrt(radial)
                cap_blend = 0.28 * smoothstep(0.60, 0.72, -point.x)
                point.x += (rounded_x - point.x) * cap_blend
            sole = smoothstep(0.885, 0.935, -point.z)
            point.z += (-0.974 - point.z) * (0.32 * left_weight * sole)

        if right_weight > 1e-5:
            point.x += (point.x - 0.04) * (0.15 * right_toe + 0.08 * right_heel)
            point.z += (point.z + 0.83) * (0.08 * right_toe + 0.055 * right_heel)
            point += vertex.normal.normalized() * (0.010 * right_weight)
            if point.y < -0.37:
                radial = max(0.0, 1.0 - ((point.x - 0.04) / 0.24) ** 2 - ((point.z + 0.83) / 0.20) ** 2)
                rounded_y = -0.40 - 0.24 * math.sqrt(radial)
                cap_blend = 0.24 * smoothstep(0.35, 0.49, -point.y)
                point.y += (rounded_y - point.y) * cap_blend
            sole = smoothstep(0.885, 0.935, -point.z)
            point.z += (-0.974 - point.z) * (0.32 * right_weight * sole)

        vertex.co = point

    mesh.update()
    for polygon in mesh.polygons:
        polygon.use_smooth = True


def create_contact_shadow() -> bpy.types.Object:
    size = 64
    image = bpy.data.images.new("YaniContactShadowMask", width=size, height=size, alpha=True)
    pixels: list[float] = []
    for y in range(size):
        for x in range(size):
            dx = (x + 0.5) / size * 2.0 - 1.0
            dy = (y + 0.5) / size * 2.0 - 1.0
            radius = min(1.0, math.sqrt(dx * dx + dy * dy))
            alpha = max(0.0, 1.0 - radius)
            pixels.extend((0.045, 0.11, 0.068, alpha * alpha * 0.46))
    image.pixels = pixels
    image.pack()

    material = bpy.data.materials.new("YaniContactShadowMaterial")
    material.use_nodes = True
    material.diffuse_color = (0.045, 0.11, 0.068, 0.46)
    material.use_backface_culling = False
    if hasattr(material, "surface_render_method"):
        material.surface_render_method = "DITHERED"
    nodes = material.node_tree.nodes
    links = material.node_tree.links
    bsdf = nodes.get("Principled BSDF")
    texture = nodes.new("ShaderNodeTexImage")
    texture.image = image
    texture.interpolation = "Linear"
    links.new(texture.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(texture.outputs["Alpha"], bsdf.inputs["Alpha"])
    bsdf.inputs["Roughness"].default_value = 1.0

    mesh = bpy.data.meshes.new("YaniContactShadowMesh")
    mesh.from_pydata(
        [(-0.88, -0.55, -0.984), (0.70, -0.55, -0.984), (0.70, 0.48, -0.984), (-0.88, 0.48, -0.984)],
        [],
        [(0, 1, 2, 3)],
    )
    mesh.materials.append(material)
    uv_layer = mesh.uv_layers.new(name="UVMap")
    uv_coordinates = ((0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0))
    for loop in mesh.loops:
        uv_layer.data[loop.index].uv = uv_coordinates[loop.vertex_index]

    shadow = bpy.data.objects.new("YaniContactShadow", mesh)
    bpy.context.collection.objects.link(shadow)
    shadow["yani_role"] = "contact_shadow"
    return shadow


def create_rig(model: bpy.types.Object) -> bpy.types.Object:
    armature_data = bpy.data.armatures.new("YaniRig")
    armature = bpy.data.objects.new("YaniRig", armature_data)
    bpy.context.collection.objects.link(armature)
    armature.show_in_front = True
    armature["yani_asset"] = "character_rig"
    armature["yani_asset_version"] = 3

    bpy.context.view_layer.objects.active = armature
    armature.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")

    def bone(name: str, head: tuple[float, float, float], tail: tuple[float, float, float], parent=None):
        entry = armature_data.edit_bones.new(name)
        entry.head = head
        entry.tail = tail
        entry.parent = parent
        entry.use_connect = False
        return entry

    root = bone("Root", (0.0, 0.0, -0.96), (0.0, 0.0, -0.72))
    body = bone("Body", (0.0, 0.0, -0.72), (0.0, 0.0, 0.18), root)
    head = bone("Head", (0.0, -0.03, 0.12), (0.0, -0.03, 0.72), body)
    bone("Ear_L", (-0.22, -0.01, 0.56), (-0.34, 0.0, 0.96), head)
    bone("Ear_R", (0.20, 0.0, 0.54), (0.36, 0.02, 0.88), head)
    bone("Leg_L", (-0.13, 0.02, -0.40), (-0.48, 0.02, -0.80), body)
    bone("Leg_R", (0.10, -0.02, -0.40), (0.20, -0.18, -0.82), body)
    tail_base = bone("Tail_Base", (0.34, 0.13, -0.30), (0.56, 0.25, -0.10), body)
    bone("Tail_Tip", (0.56, 0.25, -0.10), (0.73, 0.34, 0.16), tail_base)
    bone("Smoke", (-0.36, -0.50, 0.08), (-0.62, -0.54, 0.40), head)

    bpy.ops.object.mode_set(mode="OBJECT")
    modifier = model.modifiers.new("YaniRig", "ARMATURE")
    modifier.object = armature
    model.parent = armature

    groups = {name: model.vertex_groups.new(name=name) for name in (
        "Body", "Head", "Ear_L", "Ear_R", "Leg_L", "Leg_R", "Tail_Base", "Tail_Tip", "Smoke"
    )}
    tail_start = Vector((0.34, 0.13, -0.30))
    tail_end = Vector((0.76, 0.36, 0.17))

    for vertex in model.data.vertices:
        point = vertex.co
        head_radius = math.sqrt(point.x * point.x + (point.y + 0.04) ** 2)
        head_weight = smoothstep(-0.06, 0.25, point.z) * (1.0 - smoothstep(0.54, 0.75, head_radius))

        left_ear = ellipsoid_weight(point, Vector((-0.30, 0.0, 0.76)), Vector((0.24, 0.29, 0.34)))
        left_ear *= smoothstep(0.50, 0.69, point.z) * smoothstep(0.05, 0.20, -point.x)
        right_ear = ellipsoid_weight(point, Vector((0.30, 0.01, 0.73)), Vector((0.24, 0.29, 0.31)))
        right_ear *= smoothstep(0.47, 0.66, point.z) * smoothstep(0.05, 0.20, point.x)

        tail_weight = smoothstep(0.30, 0.49, point.x)
        tail_weight *= smoothstep(0.04, 0.19, point.y)
        tail_weight *= smoothstep(-0.40, -0.20, point.z)
        tail_weight *= 1.0 - smoothstep(0.27, 0.43, point.z)

        smoke_weight = smoothstep(0.30, 0.48, -point.x)
        smoke_weight *= smoothstep(0.34, 0.50, -point.y)
        smoke_weight *= smoothstep(-0.12, 0.05, point.z)

        lower_gate = 1.0 - smoothstep(-0.46, -0.31, point.z)
        left_leg = max(
            capsule_weight(point, Vector((-0.13, 0.02, -0.43)), Vector((-0.43, -0.01, -0.79)), 0.275),
            capsule_weight(point, Vector((-0.43, -0.01, -0.79)), Vector((-0.72, -0.15, -0.84)), 0.255),
            ellipsoid_weight(point, Vector((-0.66, -0.13, -0.83)), Vector((0.24, 0.34, 0.24)), inner=0.45),
        ) * lower_gate
        right_leg = max(
            capsule_weight(point, Vector((0.10, -0.01, -0.43)), Vector((0.10, -0.22, -0.78)), 0.275),
            capsule_weight(point, Vector((0.10, -0.22, -0.78)), Vector((0.04, -0.50, -0.84)), 0.255),
            ellipsoid_weight(point, Vector((0.04, -0.45, -0.84)), Vector((0.28, 0.27, 0.24)), inner=0.45),
        ) * lower_gate
        left_leg *= 1.0 - max(tail_weight, smoke_weight)
        right_leg *= 1.0 - max(tail_weight, smoke_weight)

        feature_total = max(left_ear, right_ear, left_leg, right_leg, tail_weight, smoke_weight)
        head_effective = head_weight * (1.0 - feature_total)
        body_weight = max(0.0, 1.0 - max(feature_total, head_effective))

        weights: dict[str, float] = {"Body": body_weight, "Head": head_effective}
        if left_ear > 0:
            weights["Ear_L"] = left_ear
            weights["Head"] += max(0.0, head_weight - left_ear) * 0.35
        if right_ear > 0:
            weights["Ear_R"] = right_ear
            weights["Head"] += max(0.0, head_weight - right_ear) * 0.35
        if left_leg > 0:
            weights["Leg_L"] = left_leg
        if right_leg > 0:
            weights["Leg_R"] = right_leg
        if tail_weight > 0:
            tail_t = segment_projection(point, tail_start, tail_end)
            tip_share = smoothstep(0.34, 0.82, tail_t)
            weights["Tail_Base"] = tail_weight * (1.0 - tip_share)
            weights["Tail_Tip"] = tail_weight * tip_share
        if smoke_weight > 0:
            weights["Smoke"] = smoke_weight

        total = sum(weights.values())
        if total < 1e-6:
            weights = {"Body": 1.0}
            total = 1.0
        for name, weight in weights.items():
            if weight > 1e-5:
                groups[name].add([vertex.index], weight / total, "REPLACE")

    return armature


def reset_pose(armature: bpy.types.Object) -> None:
    for pose_bone in armature.pose.bones:
        pose_bone.rotation_mode = "XYZ"
        pose_bone.location = (0.0, 0.0, 0.0)
        pose_bone.rotation_euler = (0.0, 0.0, 0.0)
        pose_bone.scale = (1.0, 1.0, 1.0)


def create_action(
    armature: bpy.types.Object,
    name: str,
    frames: list[tuple[int, dict[str, dict[str, tuple[float, float, float]]]]],
) -> bpy.types.Action:
    action = bpy.data.actions.new(name=name)
    action.use_fake_user = True
    armature.animation_data_create()
    armature.animation_data.action = action
    for frame, bone_values in frames:
        reset_pose(armature)
        for bone_name, values in bone_values.items():
            pose_bone = armature.pose.bones[bone_name]
            if "location" in values:
                pose_bone.location = values["location"]
            if "rotation" in values:
                pose_bone.rotation_euler = values["rotation"]
            if "scale" in values:
                pose_bone.scale = values["scale"]
            for data_path in values:
                target = "rotation_euler" if data_path == "rotation" else data_path
                pose_bone.keyframe_insert(data_path=target, frame=frame, group=bone_name)
    return action


def create_animations(armature: bpy.types.Object) -> dict[str, bpy.types.Action]:
    r = math.radians
    actions = {
        "Idle": create_action(
            armature,
            "Idle",
            [
                (0, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, 0)}, "Tail_Tip": {"rotation": (0, 0, 0)}, "Smoke": {"rotation": (0, 0, 0)}}),
                (24, {"Body": {"location": (0, 0, 0.004)}, "Ear_L": {"rotation": (r(0.8), 0, r(1.4))}, "Ear_R": {"rotation": (r(-0.4), 0, r(-0.5))}, "Leg_L": {"location": (0, 0, 0.012)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(2.5))}, "Tail_Tip": {"rotation": (0, 0, r(-4))}, "Smoke": {"rotation": (0, 0, r(2))}}),
                (48, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (r(-0.5), 0, r(-0.8))}, "Ear_R": {"rotation": (r(0.8), 0, r(1.2))}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.012)}, "Tail_Base": {"rotation": (0, 0, r(-1.5))}, "Tail_Tip": {"rotation": (0, 0, r(3))}, "Smoke": {"rotation": (0, 0, r(-1.5))}}),
                (60, {"Body": {"location": (0, 0, 0.003)}, "Ear_L": {"rotation": (r(-2.2), 0, r(-3.5))}, "Ear_R": {"rotation": (r(0.2), 0, 0)}, "Leg_L": {"location": (0, 0, 0.008)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(1))}, "Tail_Tip": {"rotation": (0, 0, r(-2))}, "Smoke": {"rotation": (0, 0, r(1))}}),
                (66, {"Body": {"location": (0, 0, 0.003)}, "Ear_L": {"rotation": (r(1.6), 0, r(2.8))}, "Ear_R": {"rotation": (r(-0.2), 0, 0)}, "Leg_L": {"location": (0, 0, 0.006)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(1.5))}, "Tail_Tip": {"rotation": (0, 0, r(-2.5))}, "Smoke": {"rotation": (0, 0, r(1.2))}}),
                (72, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.008)}, "Tail_Base": {"rotation": (0, 0, 0)}, "Tail_Tip": {"rotation": (0, 0, 0)}, "Smoke": {"rotation": (0, 0, 0)}}),
                (96, {"Body": {"location": (0, 0, 0.004)}, "Ear_L": {"rotation": (r(0.2), 0, 0)}, "Ear_R": {"rotation": (r(-2), 0, r(3.2))}, "Leg_L": {"location": (0, 0, 0.011)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(-2))}, "Tail_Tip": {"rotation": (0, 0, r(3.5))}, "Smoke": {"rotation": (0, 0, r(-1.5))}}),
                (102, {"Body": {"location": (0, 0, 0.004)}, "Ear_L": {"rotation": (r(-0.2), 0, 0)}, "Ear_R": {"rotation": (r(1.4), 0, r(-2.4))}, "Leg_L": {"location": (0, 0, 0.008)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(-1))}, "Tail_Tip": {"rotation": (0, 0, r(2))}, "Smoke": {"rotation": (0, 0, r(-1))}}),
                (120, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, 0)}, "Tail_Tip": {"rotation": (0, 0, 0)}, "Smoke": {"rotation": (0, 0, 0)}}),
            ],
        ),
        "Active": create_action(
            armature,
            "Active",
            [
                (0, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(-2))}, "Tail_Tip": {"rotation": (0, 0, r(3))}}),
                (18, {"Body": {"location": (0, 0, 0.006)}, "Ear_L": {"rotation": (r(1.5), 0, r(3))}, "Ear_R": {"rotation": (r(-0.8), 0, r(-2))}, "Leg_L": {"location": (0, 0, 0.018)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(5))}, "Tail_Tip": {"rotation": (0, 0, r(-7))}}),
                (36, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (r(-1), 0, r(-2))}, "Ear_R": {"rotation": (r(1.4), 0, r(2.5))}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.018)}, "Tail_Base": {"rotation": (0, 0, r(-4))}, "Tail_Tip": {"rotation": (0, 0, r(7))}}),
                (54, {"Body": {"location": (0, 0, 0.005)}, "Ear_L": {"rotation": (r(1), 0, r(1.8))}, "Ear_R": {"rotation": (r(-0.8), 0, r(-1.2))}, "Leg_L": {"location": (0, 0, 0.015)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(4))}, "Tail_Tip": {"rotation": (0, 0, r(-6))}}),
                (72, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(-2))}, "Tail_Tip": {"rotation": (0, 0, r(3))}}),
            ],
        ),
        "Scanning": create_action(
            armature,
            "Scanning",
            [
                (0, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (r(0.5), 0, r(4))}, "Ear_R": {"rotation": (r(-0.5), 0, r(-3.5))}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.007)}, "Tail_Base": {"rotation": (0, 0, r(-3))}, "Tail_Tip": {"rotation": (0, 0, r(5))}}),
                (18, {"Body": {"location": (0, 0, 0.003)}, "Ear_L": {"rotation": (r(-1.5), 0, r(-2.5))}, "Ear_R": {"rotation": (r(1.5), 0, r(4.5))}, "Leg_L": {"location": (0, 0, 0.008)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(3))}, "Tail_Tip": {"rotation": (0, 0, r(-5))}}),
                (36, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (r(0.8), 0, r(2.5))}, "Ear_R": {"rotation": (r(-0.8), 0, r(-2))}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.008)}, "Tail_Base": {"rotation": (0, 0, r(-2))}, "Tail_Tip": {"rotation": (0, 0, r(4))}}),
                (54, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (r(0.5), 0, r(4))}, "Ear_R": {"rotation": (r(-0.5), 0, r(-3.5))}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.007)}, "Tail_Base": {"rotation": (0, 0, r(-3))}, "Tail_Tip": {"rotation": (0, 0, r(5))}}),
            ],
        ),
        "Alarm": create_action(
            armature,
            "Alarm",
            [
                (0, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, 0)}, "Tail_Tip": {"rotation": (0, 0, 0)}}),
                (8, {"Body": {"location": (0, 0, -0.004)}, "Ear_L": {"rotation": (r(-5), 0, r(-7))}, "Ear_R": {"rotation": (r(-5), 0, r(7))}, "Leg_L": {"location": (0, 0, 0.020)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(-7))}, "Tail_Tip": {"rotation": (0, 0, r(10))}}),
                (20, {"Body": {"location": (0, 0, 0.004)}, "Ear_L": {"rotation": (r(-2.5), 0, r(-4))}, "Ear_R": {"rotation": (r(-2.5), 0, r(4))}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0.020)}, "Tail_Base": {"rotation": (0, 0, r(-4))}, "Tail_Tip": {"rotation": (0, 0, r(7))}}),
                (32, {"Body": {"location": (0, 0, -0.003)}, "Ear_L": {"rotation": (r(-4), 0, r(-6))}, "Ear_R": {"rotation": (r(-4), 0, r(6))}, "Leg_L": {"location": (0, 0, 0.016)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, r(-6))}, "Tail_Tip": {"rotation": (0, 0, r(9))}}),
                (48, {"Body": {"location": (0, 0, 0)}, "Ear_L": {"rotation": (0, 0, 0)}, "Ear_R": {"rotation": (0, 0, 0)}, "Leg_L": {"location": (0, 0, 0)}, "Leg_R": {"location": (0, 0, 0)}, "Tail_Base": {"rotation": (0, 0, 0)}, "Tail_Tip": {"rotation": (0, 0, 0)}}),
            ],
        ),
    }
    armature.animation_data.action = actions["Idle"]
    return actions


def setup_preview() -> None:
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 800
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(PREVIEW_PNG)
    scene.render.film_transparent = False
    scene.world.color = (0.015, 0.021, 0.019)

    camera_data = bpy.data.cameras.new("PreviewCamera")
    camera = bpy.data.objects.new("PreviewCamera", camera_data)
    bpy.context.collection.objects.link(camera)
    camera.location = (3.4, -3.4, 2.4)
    camera.rotation_euler = (Vector((0.0, 0.0, 0.0)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.lens = 72
    scene.camera = camera

    lights = [
        ("MintKey", (-3.2, -4.0, 5.2), (0.67, 0.94, 0.80), 900.0, 4.0),
        ("WarmFill", (3.5, -2.5, 2.0), (1.0, 0.54, 0.29), 520.0, 3.0),
        ("Rim", (2.0, 3.5, 3.8), (0.42, 0.84, 0.66), 760.0, 3.0),
    ]
    for name, location, color, energy, size in lights:
        data = bpy.data.lights.new(name, "AREA")
        data.color = color
        data.energy = energy
        data.shape = "DISK"
        data.size = size
        light = bpy.data.objects.new(name, data)
        bpy.context.collection.objects.link(light)
        light.location = location
        light.rotation_euler = (Vector((0.0, 0.0, 0.0)) - light.location).to_track_quat("-Z", "Y").to_euler()


def export_character(model: bpy.types.Object, armature: bpy.types.Object, shadow: bpy.types.Object) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    model.select_set(True)
    armature.select_set(True)
    shadow.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=str(OUTPUT_GLB),
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
    source = DEFAULT_SOURCE
    if not source.exists():
        raise FileNotFoundError(source)
    OUTPUT_BLEND.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_GLB.parent.mkdir(parents=True, exist_ok=True)
    clear_scene()
    model = import_character(source)
    optimize_textures()
    reshape_feet(model)
    armature = create_rig(model)
    shadow = create_contact_shadow()
    actions = create_animations(armature)
    bpy.context.scene.render.fps = FPS
    setup_preview()
    armature.animation_data.action = actions["Idle"]
    bpy.context.scene.frame_set(24)
    bpy.ops.render.render(write_still=True)
    bpy.ops.file.pack_all()
    bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT_BLEND))
    export_character(model, armature, shadow)

    model.data.calc_loop_triangles()
    print(f"YANI_CHARACTER_SOURCE={source}")
    print(f"YANI_CHARACTER_OUTPUT={OUTPUT_GLB}")
    print(f"YANI_CHARACTER_BLEND={OUTPUT_BLEND}")
    print(f"YANI_CHARACTER_PREVIEW={PREVIEW_PNG}")
    print(f"YANI_CHARACTER_VERTICES={len(model.data.vertices)}")
    print(f"YANI_CHARACTER_TRIANGLES={len(model.data.loop_triangles)}")
    print(f"YANI_CHARACTER_SHADOW_TRIANGLES={len(shadow.data.polygons) * 2}")
    print(f"YANI_CHARACTER_GLB_BYTES={OUTPUT_GLB.stat().st_size}")
    print(f"YANI_CHARACTER_ACTIONS={','.join(actions)}")


if __name__ == "__main__":
    main()
