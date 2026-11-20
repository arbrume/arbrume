"""Steps of the in-Blender test for envelopes, obstacles and growth frames.

Imported by ``run_in_blender.py`` inside Blender; every step takes the extension module and
the ``check`` function of the runner.
"""

from __future__ import annotations

import time

import bpy
import numpy as np

SPHERE_CENTER = (0.0, 0.0, 2.2)
SPHERE_RADIUS = 1.2
WALL_LOW, WALL_HIGH = (0.4, -3.0, 0.0), (0.7, 3.0, 4.0)


def add_sphere(name: str, center, radius: float) -> bpy.types.Object:
    """A UV sphere object."""
    bpy.ops.mesh.primitive_uv_sphere_add(radius=radius, location=center, segments=32)
    obj = bpy.context.active_object
    obj.name = name
    return obj


def add_box(name: str, low, high) -> bpy.types.Object:
    """A cube object stretched from ``low`` to ``high`` (world space)."""
    low, high = np.array(low), np.array(high)
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=tuple((low + high) / 2))
    obj = bpy.context.active_object
    obj.scale = tuple(high - low)
    obj.name = name
    return obj


def world_vertices(obj: bpy.types.Object) -> np.ndarray:
    """Vertex positions of the mesh of ``obj`` in world space."""
    coords = np.empty(3 * len(obj.data.vertices), dtype=np.float64)
    obj.data.vertices.foreach_get("co", coords)
    matrix = np.array(obj.matrix_world)
    return coords.reshape(-1, 3) @ matrix[:3, :3].T + matrix[:3, 3]


def generated_tree(ext, obj):
    """The tree the generator makes for a plant, with its surroundings."""
    settings = obj.arbrume
    around, _ = ext.plant.surroundings_of(obj)
    overrides = ext.logic.values.load_overrides(settings.overrides)
    return ext.logic.species.generate(
        ext.plant.source_of(settings),
        overrides,
        surroundings=around,
        **ext.plant.generation_args(settings),
    )


def new_plant(preset: str, seed: int, age: int, quality: float = 0.5) -> bpy.types.Object:
    """Adds a plant at the world origin and returns it."""
    bpy.context.scene.cursor.location = (0.0, 0.0, 0.0)
    result = bpy.ops.arbrume.add_plant(
        preset=preset, seed=seed, use_species_age=False, age=age, quality=quality
    )
    assert result == {"FINISHED"}
    return bpy.context.view_layer.objects.active


def test_envelope(ext, check) -> bpy.types.Object:
    """A sphere envelope keeps the crown inside; a second regeneration uses the cache."""
    plant = new_plant("envelope_tree", seed=1, age=12)
    alone = world_vertices(plant)
    sphere = add_sphere("envelope", SPHERE_CENTER, SPHERE_RADIUS)
    plant.arbrume.envelope = sphere
    status = ext.state.status_of(plant.name)
    check(status.voxelized == 1 and status.filled > 0, f"envelope voxelized: {status.filled}")
    points = world_vertices(plant)
    crown = points[:, 2] > SPHERE_CENTER[2] - SPHERE_RADIUS
    distance = np.linalg.norm(points[crown] - SPHERE_CENTER, axis=1)
    reach = SPHERE_RADIUS + 2.0 * status.cell + 0.3
    check(distance.max() < reach, f"crown inside the sphere: {distance.max():.2f} < {reach:.2f}")
    check(not plant.arbrume.envelope_ground, "trunk passage off by default")
    top = points[:, 2].max()
    check(top > SPHERE_CENTER[2], f"trunk rises into the floating sphere: top {top:.2f}")
    check(np.ptp(alone[:, 2]) > np.ptp(points[:, 2]), "envelope makes the plant smaller")
    start = time.perf_counter()
    plant.arbrume.seed = 2
    elapsed = (time.perf_counter() - start) * 1e3
    check(status.cached == 1 and status.voxelized == 0, "slider change reuses the voxels")
    print(
        f"envelope: {status.filled} voxels of {status.cell:.3f} m, surroundings "
        f"{status.surroundings_ms:.1f} ms cached, regeneration {elapsed:.0f} ms",
        flush=True,
    )
    sphere.location.x += 0.3
    plant.arbrume.seed = 1
    check(status.voxelized == 1, "moving the envelope voxelizes it again")
    check(len(plant.data.polygons) == generated_tree(ext, plant).stats["triangles"], "counts")
    sphere.location.x -= 0.3
    return plant


def in_wall(points: np.ndarray) -> np.ndarray:
    """Which world points lie in the wall."""
    low, high = np.array(WALL_LOW), np.array(WALL_HIGH)
    return np.all((points > low) & (points < high), axis=1)


def test_obstacle(ext, check) -> None:
    """A cube obstacle is avoided; a parametric species ignores it."""
    plant = new_plant("open_grown_broadleaf", seed=2, age=14)
    nodes = generated_tree(ext, plant).skeleton.positions
    to_world = np.stack([nodes[:, 0], -nodes[:, 2], nodes[:, 1]], axis=1)
    check(in_wall(to_world).any(), "without the wall the plant grows through it")
    wall = add_box("wall", WALL_LOW, WALL_HIGH)
    bpy.context.view_layer.objects.active = plant
    for other in bpy.context.selected_objects:
        other.select_set(False)
    wall.select_set(True)
    plant.select_set(True)
    check(bpy.ops.arbrume.add_obstacle() == {"FINISHED"}, "add_obstacle")
    check(plant.arbrume.obstacles[0].obj == wall, "the selected cube is the obstacle")
    status = ext.state.status_of(plant.name)
    print(f"wall: {status.filled} voxels in {status.surroundings_ms:.1f} ms", flush=True)
    nodes = generated_tree(ext, plant).skeleton.positions
    to_world = np.stack([nodes[:, 0], -nodes[:, 2], nodes[:, 1]], axis=1)
    check(not in_wall(to_world).any(), "with the wall no node grows inside it")
    check(len(plant.data.polygons) == generated_tree(ext, plant).stats["triangles"], "wall mesh")
    plant.arbrume.preset = "shrub"
    around, _ = ext.plant.surroundings_of(plant)
    check(around is None, "a parametric species gets no surroundings")
    check(bpy.ops.arbrume.remove_obstacle(index=0) == {"FINISHED"}, "remove_obstacle")
    check(len(plant.arbrume.obstacles) == 0, "obstacle removed")
    for obj in (plant, wall):
        bpy.data.objects.remove(obj)


def test_growth_frames(ext, check, plant) -> None:
    """Growth frames are objects of a new collection, shown one at a time."""
    scene = bpy.context.scene
    scene.frame_start = 1
    bpy.context.view_layer.objects.active = plant
    stages, step = 4, 3
    start = time.perf_counter()
    result = bpy.ops.arbrume.growth_frames(stages=stages, step=step)
    elapsed = (time.perf_counter() - start) * 1e3
    check(result == {"FINISHED"}, f"growth_frames ({elapsed:.0f} ms)")
    collection = bpy.data.collections.get(f"{plant.name} growth")
    objects = sorted(collection.objects, key=lambda o: o.name) if collection else []
    check(len(objects) == stages, f"{len(objects)} growth objects")
    cycles = [o["arbrume_growth_cycle"] for o in objects]
    check(cycles == sorted(cycles) and cycles[-1] == plant.arbrume.age - 1, f"cycles {cycles}")
    sizes = [len(o.data.polygons) for o in objects]
    check(sizes == sorted(sizes) and sizes[0] < sizes[-1], f"stages grow: {sizes}")
    check(plant.hide_get() and plant.hide_render, "the plant itself is hidden")
    visible_ok = True
    for frame in range(1, stages * step + 4):
        scene.frame_set(frame)
        shown = [i for i, o in enumerate(objects) if not o.hide_viewport]
        rendered = [i for i, o in enumerate(objects) if not o.hide_render]
        expected = [ext.logic.frames.visible_stage(stages, 1, step, frame)]
        visible_ok &= shown == rendered == expected
    check(visible_ok, "one growth stage visible at each frame, in order")
    check(scene.frame_end >= stages * step, "scene long enough for every stage")
    scene.frame_set(1)
    for obj in objects:
        bpy.data.objects.remove(obj)
    bpy.data.collections.remove(collection)
    plant.hide_set(False)
    plant.hide_render = False


def run(ext, check) -> None:
    """Every step, in a scene of their own objects."""
    plant = test_envelope(ext, check)
    test_growth_frames(ext, check, plant)
    sphere = plant.arbrume.envelope
    for obj in (plant, sphere):
        bpy.data.objects.remove(obj)
    test_obstacle(ext, check)


def render_still(ext, out) -> None:
    """A plant grown inside a sphere envelope next to a wall, rendered into ``out``; a
    wireframe copy of the envelope shows it. Every other plant is hidden from the render."""
    for obj in bpy.data.objects:
        if getattr(obj, "arbrume", None) is not None and obj.arbrume.is_plant:
            obj.hide_render = True
    plant = new_plant("envelope_tree", seed=3, age=16, quality=1.0)
    sphere = add_sphere("envelope", SPHERE_CENTER, SPHERE_RADIUS)
    sphere.display_type = "WIRE"
    sphere.hide_render = True
    shown = add_sphere("envelope shown", SPHERE_CENTER, SPHERE_RADIUS)
    shown.modifiers.new("wire", "WIREFRAME").thickness = 0.008
    wall = add_box("wall", (0.9, -3.0, 0.0), (1.1, 3.0, 4.0))
    material = bpy.data.materials.new("wall")
    material.diffuse_color = (0.6, 0.58, 0.55, 1.0)
    wall.data.materials.append(material)
    bpy.context.view_layer.objects.active = plant
    plant.arbrume.envelope = sphere
    plant.arbrume.obstacles.add().obj = wall
    ext.plant.regenerate(plant)
    status = ext.state.status_of(plant.name)
    print(f"render plant: {status.triangles} triangles, {status.filled} voxels", flush=True)
    scene = bpy.context.scene
    camera = scene.camera
    camera.location = (-4.5, -5.5, 2.6)
    target = type(camera.location)((0.2, 0.0, 1.9))
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out / "surroundings.png")
    bpy.ops.render.render(write_still=True)
