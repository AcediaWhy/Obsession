from __future__ import annotations

import json
import os
from pathlib import Path

import bpy
from mathutils import Vector


PROJECT_ROOT = Path(__file__).resolve().parents[2]
OUTPUT_DIR = PROJECT_ROOT / "assets-src" / "yani" / "diagnostics" / os.environ.get("YANI_FEET_DIAGNOSTIC", "feet-before")


def weighted_vertices(model: bpy.types.Object, group_name: str, threshold: float = 0.35):
    group = model.vertex_groups.get(group_name)
    if group is None:
        return []
    result = []
    for vertex in model.data.vertices:
        try:
            weight = group.weight(vertex.index)
        except RuntimeError:
            continue
        if weight >= threshold:
            result.append((vertex, weight))
    return result


def group_stats(model: bpy.types.Object, group_name: str) -> dict[str, object]:
    weighted = weighted_vertices(model, group_name)
    points = [model.matrix_world @ vertex.co for vertex, _ in weighted]
    if not points:
        return {"count": 0}
    minimum = Vector((min(point.x for point in points), min(point.y for point in points), min(point.z for point in points)))
    maximum = Vector((max(point.x for point in points), max(point.y for point in points), max(point.z for point in points)))
    extents = maximum - minimum
    bottom_band = [point for point in points if point.z <= minimum.z + 0.025]
    return {
        "count": len(points),
        "min": [round(value, 5) for value in minimum],
        "max": [round(value, 5) for value in maximum],
        "extents": [round(value, 5) for value in extents],
        "bottom_band_count": len(bottom_band),
        "mean_weight": round(sum(weight for _, weight in weighted) / len(weighted), 5),
    }


def point_camera(camera: bpy.types.Object, target: Vector) -> None:
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()


def render_view(scene: bpy.types.Scene, camera: bpy.types.Object, name: str, location: tuple[float, float, float], target: Vector, frame: int) -> None:
    camera.location = location
    point_camera(camera, target)
    scene.frame_set(frame)
    scene.render.filepath = str(OUTPUT_DIR / f"{name}-f{frame:03d}.png")
    bpy.ops.render.render(write_still=True)


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    model = bpy.data.objects.get("YaniCharacter")
    armature = bpy.data.objects.get("YaniRig")
    if model is None or armature is None:
        raise RuntimeError("YaniCharacter/YaniRig not found in the open blend")

    model.data.calc_loop_triangles()
    all_points = [model.matrix_world @ vertex.co for vertex in model.data.vertices]
    bounds_min = Vector((min(point.x for point in all_points), min(point.y for point in all_points), min(point.z for point in all_points)))
    bounds_max = Vector((max(point.x for point in all_points), max(point.y for point in all_points), max(point.z for point in all_points)))
    stats = {
        "vertices": len(model.data.vertices),
        "triangles": len(model.data.loop_triangles),
        "bounds_min": [round(value, 5) for value in bounds_min],
        "bounds_max": [round(value, 5) for value in bounds_max],
        "Leg_L": group_stats(model, "Leg_L"),
        "Leg_R": group_stats(model, "Leg_R"),
        "actions": sorted(action.name for action in bpy.data.actions),
    }
    print("YANI_FEET_STATS=" + json.dumps(stats, ensure_ascii=False, sort_keys=True))

    scene = bpy.context.scene
    scene.render.resolution_x = 640
    scene.render.resolution_y = 640
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.film_transparent = False
    scene.world.color = (0.055, 0.075, 0.064)

    camera_data = bpy.data.cameras.new("FeetDiagnosticCamera")
    camera_data.type = "ORTHO"
    camera_data.ortho_scale = 1.55
    camera = bpy.data.objects.new("FeetDiagnosticCamera", camera_data)
    bpy.context.collection.objects.link(camera)
    scene.camera = camera
    target = Vector((0.0, 0.0, -0.59))

    render_view(scene, camera, "front", (0.0, -3.2, -0.52), target, 0)
    render_view(scene, camera, "front", (0.0, -3.2, -0.52), target, 24)
    render_view(scene, camera, "side", (3.0, -0.05, -0.52), target, 0)
    render_view(scene, camera, "bottom", (0.0, 0.02, -3.1), target, 0)
    print(f"YANI_FEET_DIAGNOSTICS={OUTPUT_DIR}")


if __name__ == "__main__":
    main()
