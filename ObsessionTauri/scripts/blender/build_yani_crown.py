from __future__ import annotations

import math
from pathlib import Path

import bpy
from mathutils import Vector


PROJECT_ROOT = Path(__file__).resolve().parents[2]
SOURCE_BLEND = PROJECT_ROOT / "assets-src" / "yani" / "yani-crown.blend"
PREVIEW_PNG = PROJECT_ROOT / "assets-src" / "yani" / "yani-crown-preview.png"
OUTPUT_GLB = PROJECT_ROOT / "public" / "yani" / "yani-crown.glb"
EAR_REFERENCE = PROJECT_ROOT / "public" / "yani" / "ear-surface.gltf"

RING_SIDES = 8
PATH_STEPS = 10


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (bpy.data.meshes, bpy.data.curves, bpy.data.materials, bpy.data.cameras, bpy.data.lights):
        for datablock in list(datablocks):
            if datablock.users == 0:
                datablocks.remove(datablock)


def principled_material(name: str, color: tuple[float, float, float, float], roughness: float):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    material.diffuse_color = color
    bsdf = material.node_tree.nodes.get("Principled BSDF")
    if bsdf:
        if base_color := bsdf.inputs.get("Base Color"):
            base_color.default_value = color
        if roughness_input := bsdf.inputs.get("Roughness"):
            roughness_input.default_value = roughness
        if metallic := bsdf.inputs.get("Metallic"):
            metallic.default_value = 0.0
        for input_name in ("Anisotropic IOR Level", "Anisotropic"):
            anisotropy = bsdf.inputs.get(input_name)
            if anisotropy:
                anisotropy.default_value = 0.58
                break
        coat = bsdf.inputs.get("Coat Weight")
        if coat:
            coat.default_value = 0.06
    return material


def cubic(points: tuple[Vector, Vector, Vector, Vector], t: float) -> Vector:
    p0, p1, p2, p3 = points
    u = 1.0 - t
    return p0 * (u**3) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t**3)


def cubic_tangent(points: tuple[Vector, Vector, Vector, Vector], t: float) -> Vector:
    p0, p1, p2, p3 = points
    u = 1.0 - t
    tangent = (p1 - p0) * (3.0 * u * u) + (p2 - p1) * (6.0 * u * t) + (p3 - p2) * (3.0 * t * t)
    return tangent.normalized()


def profile_value(values: tuple[float, float, float, float], t: float) -> float:
    scaled = min(2.999999, max(0.0, t * 3.0))
    index = int(scaled)
    blend = scaled - index
    smooth = blend * blend * (3.0 - 2.0 * blend)
    return values[index] * (1.0 - smooth) + values[index + 1] * smooth


def scalp_normal(point: Vector) -> Vector:
    # The runtime camera looks along -Z. Hair clumps therefore grow across a
    # shallow crown shell whose normal mostly faces +Z; a small X/Y bias keeps
    # highlights curved without turning the locks into radial flower petals.
    return Vector((point.x * 0.1, 0.04 + point.y * 0.025, 1.0)).normalized()


def create_clump(
    name: str,
    points: tuple[tuple[float, float, float], ...],
    widths: tuple[float, float, float, float],
    thicknesses: tuple[float, float, float, float],
    material,
    parent,
    layer: str,
) -> bpy.types.Object:
    path = tuple(Vector(point) for point in points)
    origin = path[0].copy()
    vertices: list[tuple[float, float, float]] = []
    uvs: list[tuple[float, float]] = []
    faces: list[tuple[int, ...]] = []

    for step in range(PATH_STEPS + 1):
        t = step / PATH_STEPS
        center = cubic(path, t)
        tangent = cubic_tangent(path, t)
        outward = scalp_normal(center)
        side = tangent.cross(outward)
        if side.length_squared < 1e-7:
            side = Vector((1.0, 0.0, 0.0))
        side.normalize()
        depth = side.cross(tangent).normalized()
        if depth.dot(outward) < 0:
            depth.negate()
        width = profile_value(widths, t)
        # Hair masses need a readable overlap, not a visible tube wall. A very
        # thin elliptical section keeps the authored depth while preventing
        # black helmet-like seams under the runtime lights.
        thickness = profile_value(thicknesses, t) * 0.34
        for radial in range(RING_SIDES):
            angle = radial / RING_SIDES * math.tau
            # Slightly flatten the rear half: the clump reads as a soft hair blade,
            # not a tube, while retaining enough depth for real highlights.
            radial_depth = math.sin(angle) * thickness * (0.72 if math.sin(angle) < 0 else 1.0)
            point = center + side * (math.cos(angle) * width) + depth * radial_depth
            vertices.append(tuple(point - origin))
            uvs.append((radial / RING_SIDES, t))

    for step in range(PATH_STEPS):
        row = step * RING_SIDES
        next_row = (step + 1) * RING_SIDES
        for radial in range(RING_SIDES):
            next_radial = (radial + 1) % RING_SIDES
            faces.append((row + radial, row + next_radial, next_row + next_radial, next_row + radial))
    faces.append(tuple(reversed(tuple(range(RING_SIDES)))))
    last_row = PATH_STEPS * RING_SIDES
    faces.append(tuple(last_row + radial for radial in range(RING_SIDES)))

    mesh = bpy.data.meshes.new(f"{name}Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update(calc_edges=True)
    uv_layer = mesh.uv_layers.new(name="HairDirection")
    for polygon in mesh.polygons:
        polygon.use_smooth = True
        for loop_index in polygon.loop_indices:
            vertex_index = mesh.loops[loop_index].vertex_index
            uv_layer.data[loop_index].uv = uvs[vertex_index]

    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.location = origin
    obj.data.materials.append(material)
    obj.parent = parent
    obj["yani_role"] = "hair_clump"
    obj["yani_layer"] = layer
    return obj


def create_scalp_cap(material, parent) -> bpy.types.Object:
    rows = 24
    columns = 64
    vertices: list[tuple[float, float, float]] = []
    faces: list[tuple[int, ...]] = []
    uvs: list[tuple[float, float]] = []
    # One continuous shell, but with an authored fringe contour. The points
    # alternate between notches and tips, producing the broad, chunky locks in
    # the character reference without stacking leaf-shaped meshes on top.
    fringe_contour = (
        (-1.00, -0.25),
        (-0.90, -0.43),
        (-0.77, -0.29),
        (-0.65, -0.46),
        (-0.49, -0.24),
        (-0.34, -0.43),
        (-0.16, -0.19),
        (-0.035, -0.42),
        (0.16, -0.18),
        (0.31, -0.44),
        (0.50, -0.25),
        (0.66, -0.46),
        (0.79, -0.30),
        (0.91, -0.43),
        (1.00, -0.25),
    )
    separators = (
        (-0.29, -0.90),
        (-0.22, -0.65),
        (-0.13, -0.34),
        (0.025, -0.035),
        (0.11, 0.31),
        (0.21, 0.66),
        (0.31, 0.91),
    )

    def fringe_bottom(normalized_x: float) -> float:
        for index in range(len(fringe_contour) - 1):
            x0, y0 = fringe_contour[index]
            x1, y1 = fringe_contour[index + 1]
            if normalized_x <= x1:
                blend = (normalized_x - x0) / max(1e-6, x1 - x0)
                return y0 * (1.0 - blend) + y1 * blend
        return fringe_contour[-1][1]

    for row in range(rows + 1):
        v = row / rows
        for column in range(columns + 1):
            u = column / columns
            # The references show one compact, continuous crown: a broad round
            # top between the ears, two large side masses and a shallow part.
            # Keep everything below the crop as supporting scalp instead of
            # modelling bangs, a face or separate ribbon-like locks.
            # The crown starts in the narrow gap between the ears and opens
            # towards the forehead. A full-width dome intersects the pinnae in
            # the field crop and reads as a helmet.
            half_width = 0.50 + 0.37 * (v**0.78) + math.sin(v * math.pi) * 0.045
            x = (u * 2.0 - 1.0) * half_width
            normalized_x = abs(x) / half_width
            part_center = 0.035
            top_part = math.exp(-(((x - part_center) / 0.09) ** 2))
            top = 0.65 - 0.25 * (normalized_x ** 1.78) - 0.003 * top_part + x * 0.012
            bottom = fringe_bottom(x / half_width)
            y = top * (1.0 - v) + bottom * v
            # Two broad lobes carry the volume. The narrow recessed part bends
            # slightly left as it descends, matching the asymmetry in the anime
            # references without turning the crown into a symmetrical helmet.
            crown_envelope = max(0.0, 1.0 - normalized_x**2)
            left_mass = math.exp(-(((x + 0.38) / 0.34) ** 2))
            right_mass = math.exp(-(((x - 0.43) / 0.37) ** 2))
            flowing_part_center = part_center - v * 0.075
            flowing_part = math.exp(-(((x - flowing_part_center) / (0.058 + v * 0.026)) ** 2))
            flowing_part *= (1.0 - v) ** 0.72
            z = -0.54 + 0.38 * crown_envelope
            z += 0.064 * math.sin(v * math.pi) * (0.52 + crown_envelope * 0.48)
            z += (left_mass * 0.052 + right_mass * 0.059) * ((1.0 - v) ** 0.82)
            z -= flowing_part * 0.082

            # The ears visibly sink into two soft root masses instead of
            # floating above a flat helmet.
            socket_envelope = math.exp(-(((v - 0.12) / 0.17) ** 2))
            left_socket = math.exp(-(((x + 0.67) / 0.18) ** 2))
            right_socket = math.exp(-(((x - 0.69) / 0.18) ** 2))
            z += (left_socket + right_socket) * socket_envelope * 0.052

            # Seven separators spread from the centre part to the angular
            # notches in the fringe. They are real shallow recesses, so the
            # locks stay legible under both the Blender and runtime lighting.
            fringe_gate = max(0.0, min(1.0, (v - 0.24) / 0.62))
            fringe_gate = fringe_gate * fringe_gate * (3.0 - 2.0 * fringe_gate)
            for root_x, tip_x in separators:
                curve = v * v * (3.0 - 2.0 * v)
                seam_x = root_x * (1.0 - curve) + tip_x * curve
                seam_distance = abs(x / half_width - seam_x)
                z -= math.exp(-((seam_distance / 0.020) ** 2)) * 0.031 * fringe_gate

            # Low broad ridges keep each lock from reading as a flat board.
            ridge_phase = (x / half_width + 1.0) * 7.0
            ridge = 0.5 + 0.5 * math.cos(ridge_phase * math.pi)
            z += (ridge**3) * 0.018 * fringe_gate
            vertices.append((x, y, z))
            uvs.append((u, v))
    stride = columns + 1
    for row in range(rows):
        for column in range(columns):
            a = row * stride + column
            b = a + 1
            c = a + stride
            d = c + 1
            faces.append((a, c, d, b))

    mesh = bpy.data.meshes.new("CrownScalpCapMesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update(calc_edges=True)
    uv_layer = mesh.uv_layers.new(name="HairDirection")
    for polygon in mesh.polygons:
        polygon.use_smooth = True
        for loop_index in polygon.loop_indices:
            vertex_index = mesh.loops[loop_index].vertex_index
            uv_layer.data[loop_index].uv = uvs[vertex_index]
    obj = bpy.data.objects.new("CrownScalpCap", mesh)
    bpy.context.collection.objects.link(obj)
    obj.data.materials.append(material)
    obj.parent = parent
    obj["yani_role"] = "scalp_cap"
    return obj


def build_crown() -> tuple[bpy.types.Object, list[bpy.types.Object]]:
    shade = principled_material("YaniHairShade", (0.29, 0.45, 0.37, 1.0), 0.64)
    mint = principled_material("YaniHairMint", (0.47, 0.64, 0.55, 1.0), 0.53)
    root = bpy.data.objects.new("YaniCrownRoot", None)
    bpy.context.collection.objects.link(root)
    root["yani_asset"] = "crown"
    root["yani_asset_version"] = 7
    meshes = [create_scalp_cap(shade, root)]

    # Fine recessed separators sit just above the shell. In the actual field
    # crop the angular fringe tips are hidden by the glass card, so these lines
    # are what keep the visible upper crown reading as layered anime hair.
    seam_specs = [
        ("Back_Seam_L_Inner", ((-0.13, 0.59, -0.16), (-0.14, 0.37, -0.17), (-0.19, 0.02, -0.19), (-0.30, -0.37, -0.24))),
        ("Back_Seam_Center", ((0.015, 0.61, -0.16), (0.02, 0.38, -0.16), (-0.005, 0.02, -0.17), (-0.03, -0.37, -0.20))),
        ("Back_Seam_R_Inner", ((0.11, 0.59, -0.16), (0.13, 0.37, -0.17), (0.19, 0.02, -0.19), (0.30, -0.37, -0.24))),
    ]
    for name, points in seam_specs:
        meshes.append(
            create_clump(
                name,
                points,
                (0.0045, 0.0065, 0.0055, 0.001),
                (0.002, 0.0025, 0.002, 0.0005),
                shade,
                root,
                "back",
            ),
        )
    return root, meshes


def import_ear_reference() -> list[bpy.types.Object]:
    if not EAR_REFERENCE.exists():
        return []
    before = set(bpy.context.scene.objects)
    bpy.ops.import_scene.gltf(filepath=str(EAR_REFERENCE))
    imported = [obj for obj in bpy.context.scene.objects if obj not in before and obj.type == "MESH"]
    if not imported:
        return []

    reference_collection = bpy.data.collections.new("REFERENCE_EARS")
    bpy.context.scene.collection.children.link(reference_collection)
    for obj in imported:
        for collection in list(obj.users_collection):
            collection.objects.unlink(obj)
        reference_collection.objects.link(obj)

    left_root = bpy.data.objects.new("LeftEarReference", None)
    reference_collection.objects.link(left_root)
    left_root.location = (-0.7, 0.11, -0.1)
    left_root.rotation_euler = (-0.025, 0.075, 0.43)
    left_root.scale = (0.86, 0.873, 0.86)
    for obj in imported:
        obj.parent = left_root

    right_root = bpy.data.objects.new("RightEarReference", None)
    reference_collection.objects.link(right_root)
    right_root.location = (0.7, 0.085, -0.12)
    right_root.rotation_euler = (-0.012, -0.065, -0.43)
    right_root.scale = (0.843, 0.851, 0.86)
    for obj in imported:
        duplicate = obj.copy()
        duplicate.data = obj.data.copy()
        reference_collection.objects.link(duplicate)
        duplicate.parent = right_root
        duplicate.scale.x = -1.0
    return [left_root, right_root, *imported]


def look_at(obj: bpy.types.Object, target: Vector) -> None:
    obj.rotation_euler = (target - obj.location).to_track_quat("-Z", "Y").to_euler()


def setup_preview() -> None:
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 1000
    scene.render.resolution_y = 680
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(PREVIEW_PNG)
    scene.render.film_transparent = False
    scene.world.color = (0.018, 0.026, 0.022)

    camera_data = bpy.data.cameras.new("PreviewCamera")
    camera = bpy.data.objects.new("PreviewCamera", camera_data)
    bpy.context.collection.objects.link(camera)
    camera.location = (0.0, 0.23, 5.2)
    camera.data.lens = 72
    look_at(camera, Vector((0.0, 0.27, 0.0)))
    scene.camera = camera

    lights = [
        ("MintKey", "AREA", (-2.8, 4.5, 4.8), (0.68, 0.92, 0.81), 850.0, 4.0),
        ("WarmFill", "AREA", (2.6, -0.5, 3.0), (1.0, 0.55, 0.30), 520.0, 3.2),
        ("Rim", "AREA", (2.4, 2.2, -3.2), (0.42, 0.84, 0.65), 920.0, 3.0),
    ]
    for name, light_type, location, color, energy, size in lights:
        data = bpy.data.lights.new(name, light_type)
        data.color = color
        data.energy = energy
        data.shape = "DISK"
        data.size = size
        light = bpy.data.objects.new(name, data)
        bpy.context.collection.objects.link(light)
        light.location = location
        look_at(light, Vector((0.0, 0.2, -0.25)))


def main() -> None:
    SOURCE_BLEND.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_GLB.parent.mkdir(parents=True, exist_ok=True)
    clear_scene()
    import_ear_reference()
    root, crown_meshes = build_crown()
    setup_preview()
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE_BLEND))
    bpy.ops.render.render(write_still=True)

    bpy.ops.object.select_all(action="DESELECT")
    root.select_set(True)
    for obj in crown_meshes:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = root
    bpy.ops.export_scene.gltf(
        filepath=str(OUTPUT_GLB),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=True,
        export_animations=False,
        export_extras=True,
    )
    triangle_count = sum(len(obj.data.loop_triangles) for obj in crown_meshes if obj.type == "MESH")
    vertex_count = sum(len(obj.data.vertices) for obj in crown_meshes if obj.type == "MESH")
    print(f"YANI_CROWN_OUTPUT={OUTPUT_GLB}")
    print(f"YANI_CROWN_BLEND={SOURCE_BLEND}")
    print(f"YANI_CROWN_PREVIEW={PREVIEW_PNG}")
    print(f"YANI_CROWN_VERTICES={vertex_count}")
    print(f"YANI_CROWN_TRIANGLES={triangle_count}")


if __name__ == "__main__":
    main()
