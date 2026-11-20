"""Steps of the in-Blender test for climbing plants and their supports.

Imported by ``run_in_blender.py`` inside Blender; every step takes the extension module and
the ``check`` function of the runner. ``render`` makes the Cycles still of an ivy on a
wall with a window, outside the test run.
"""

from __future__ import annotations

import bpy
import numpy as np
from surroundings_steps import add_box, generated_tree, new_plant, world_vertices

WALL_LOW, WALL_HIGH = (-3.0, 0.12, 0.0), (3.0, 0.42, 4.0)


def to_world(nodes: np.ndarray) -> np.ndarray:
    """Points of the frame of a plant (Y up) in world space (Z up), plant at the origin."""
    return np.stack([nodes[:, 0], -nodes[:, 2], nodes[:, 1]], axis=1)


def wall_distance(points: np.ndarray) -> np.ndarray:
    """Distance from world points to the wall box, negative inside."""
    low, high = np.array(WALL_LOW), np.array(WALL_HIGH)
    outside = np.maximum(low - points, points - high)
    return np.linalg.norm(np.maximum(outside, 0.0), axis=1) + np.minimum(outside.max(axis=1), 0.0)


def select_support(plant: bpy.types.Object, wall: bpy.types.Object) -> None:
    """Makes ``wall`` the only obstacle slot of ``plant``, in the support role."""
    bpy.context.view_layer.objects.active = plant
    for other in bpy.context.selected_objects:
        other.select_set(False)
    wall.select_set(True)
    plant.select_set(True)
    assert bpy.ops.arbrume.add_obstacle() == {"FINISHED"}
    plant.arbrume.obstacles[0].role = "SUPPORT"


def test_support(ext, check) -> None:
    """Ivy given a wall as support climbs it and stays out of it; the support is cached;
    a species that neither climbs nor self-organizes gets no surroundings."""
    plant = new_plant("ivy_like", seed=2, age=10)
    alone = world_vertices(plant)
    check(alone[:, 2].max() < 0.3, f"ivy without support creeps: {alone[:, 2].max():.2f} m")
    wall = add_box("climbing wall", WALL_LOW, WALL_HIGH)
    select_support(plant, wall)
    around, _ = ext.plant.surroundings_of(plant)
    occluder = around["occluders"][0]
    check(occluder["blocks"] and occluder["shade"] == 0.0, "a support blocks without shade")
    status = ext.state.status_of(plant.name)
    print(f"support: {status.filled} voxels of {status.cell:.3f} m", flush=True)
    skeleton = generated_tree(ext, plant).skeleton
    nodes = to_world(skeleton.positions)
    distance = wall_distance(nodes)
    check(nodes[:, 2].max() > 1.5, f"ivy climbs the wall: {nodes[:, 2].max():.2f} m")
    check(distance.min() > -1e-3, f"no node inside the wall: {distance.min():.4f}")
    near = distance < 0.08 + 2.0 * status.cell
    check(near.mean() > 0.5, f"most nodes hold the wall: {near.mean():.2f}")
    check(len(plant.data.polygons) == generated_tree(ext, plant).stats["triangles"], "counts")
    plant.arbrume.seed = 3
    check(status.cached == 1 and status.voxelized == 0, "the support is voxelized once")
    plant.arbrume.preset = "shrub"
    around, _ = ext.plant.surroundings_of(plant)
    check(around is None, "a parametric shrub gets no surroundings")
    for obj in (plant, wall):
        bpy.data.objects.remove(obj)


def run(ext, check) -> None:
    """Every step."""
    test_support(ext, check)


def wall_with_window(name: str) -> bpy.types.Object:
    """A wall 6 m wide and 4 m tall with a window opening, as one closed mesh."""
    parts = [
        ((-3.0, 0.12, 0.0), (-0.4, 0.42, 4.0)),
        ((0.9, 0.12, 0.0), (3.0, 0.42, 4.0)),
        ((-0.4, 0.12, 0.0), (0.9, 0.42, 1.2)),
        ((-0.4, 0.12, 2.6), (0.9, 0.42, 4.0)),
    ]
    boxes = [add_box(f"{name} {index}", low, high) for index, (low, high) in enumerate(parts)]
    for box in boxes:
        box.select_set(True)
    bpy.context.view_layer.objects.active = boxes[0]
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    bpy.ops.object.join()
    wall = bpy.context.view_layer.objects.active
    wall.name = name
    return wall


def render(ext, out, samples: int = 64) -> None:
    """Ivy grown on a wall with a window, rendered with Cycles into ``out``."""
    for obj in list(bpy.data.objects):
        if obj.type in {"MESH", "EMPTY"}:
            bpy.data.objects.remove(obj)
    wall = wall_with_window("wall")
    material = bpy.data.materials.new("plaster")
    material.diffuse_color = (0.75, 0.7, 0.62, 1.0)
    material.use_nodes = True
    principled = material.node_tree.nodes["Principled BSDF"]
    principled.inputs["Base Color"].default_value = (0.72, 0.66, 0.58, 1.0)
    wall.data.materials.append(material)
    plant = new_plant("ivy_like", seed=4, age=16, quality=1.0)
    select_support(plant, wall)
    plant.arbrume.voxel_size = 0.04
    ext.plant.regenerate(plant)
    status = ext.state.status_of(plant.name)
    print(f"ivy: {status.triangles} triangles, {status.filled} voxels", flush=True)
    bpy.ops.mesh.primitive_plane_add(size=40.0)
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = samples
    scene.render.resolution_x, scene.render.resolution_y = 1280, 960
    camera = scene.camera
    if camera is None:
        bpy.ops.object.camera_add()
        camera = scene.camera = bpy.context.active_object
    camera.location = (-2.6, -10.5, 2.3)
    target = type(camera.location)((0.0, 0.3, 2.0))
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    world = scene.world or bpy.data.worlds.new("sky")
    scene.world = world
    world.color = (0.55, 0.65, 0.8)
    background = world.node_tree.nodes.get("Background") if world.node_tree else None
    if background is not None:
        background.inputs["Color"].default_value = (0.55, 0.65, 0.8, 1.0)
    bpy.ops.object.light_add(type="SUN", rotation=(0.9, 0.2, -0.6))
    bpy.context.active_object.data.energy = 4.0
    scene.render.filepath = str(out / "ivy_wall_cycles.png")
    bpy.ops.render.render(write_still=True)
