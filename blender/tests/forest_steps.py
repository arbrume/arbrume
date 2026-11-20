"""Steps of the in-Blender test for the *Arbrume Forest* operator.

Imported by ``run_in_blender.py`` inside Blender. A hilly grid is the terrain; the forest
must stand on it, be drawn as instances of a small library excluded from the view
layer, be the same for the same seed, and render.
"""

from __future__ import annotations

import time
from pathlib import Path

import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

SIZE = 80.0
MIX = "shrub:300, conifer:60"


def add_terrain(name: str) -> bpy.types.Object:
    """A subdivided grid with two hills, ``SIZE`` meters wide."""
    bpy.ops.mesh.primitive_grid_add(
        x_subdivisions=60, y_subdivisions=60, size=SIZE, location=(5.0, -3.0, 1.0)
    )
    obj = bpy.context.active_object
    obj.name = name
    mesh = obj.data
    co = np.empty(len(mesh.vertices) * 3, dtype=np.float32)
    mesh.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    co[:, 2] = 6.0 * np.exp(-((co[:, 0] - 10.0) ** 2 + co[:, 1] ** 2) / 300.0)
    co[:, 2] += 3.0 * np.sin(co[:, 0] / 9.0) * np.cos(co[:, 1] / 13.0)
    mesh.vertices.foreach_set("co", co.reshape(-1))
    mesh.update()
    return obj


def ground_heights(terrain: bpy.types.Object, points: np.ndarray) -> np.ndarray:
    """World height of ``terrain`` under each of ``points`` (``nan`` off the mesh)."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    tree = BVHTree.FromObject(terrain, depsgraph)
    inverse = terrain.matrix_world.inverted()
    heights = np.full(len(points), np.nan)
    for index, (x, y, _) in enumerate(points):
        origin = inverse @ Vector((float(x), float(y), 100.0))
        hit = tree.ray_cast(origin, (0.0, 0.0, -1.0))
        if hit[0] is not None:
            heights[index] = (terrain.matrix_world @ hit[0]).z
    return heights


def add_forest(terrain: bpy.types.Object, seed: int):
    """Runs the operator; returns the new instancer object."""
    before = set(bpy.data.objects)
    result = bpy.ops.arbrume.add_forest(
        terrain=terrain.name,
        mix=MIX,
        seed=seed,
        variants=2,
        lods=2,
        billboard="cross",
        textures=False,
        resolution=1.5,
        viewport_level=2,
    )
    new = [o for o in bpy.data.objects if o not in before and o.modifiers]
    return result, new[0] if new else None


def vertices(obj: bpy.types.Object) -> np.ndarray:
    co = np.empty(len(obj.data.vertices) * 3, dtype=np.float32)
    obj.data.vertices.foreach_get("co", co)
    return co.reshape(-1, 3)


def check_library(ext, check, forest: bpy.types.Object) -> None:
    """The library: excluded from the view layer, one object per variant and level."""
    library = bpy.data.collections.get(f"{forest.name} library")
    check(library is not None, "forest: the library collection exists")
    if library is None:
        return
    layer = next(
        (c for c in bpy.context.view_layer.layer_collection.children if c.collection == library),
        None,
    )
    check(
        layer is not None and layer.exclude, "forest: the library is excluded from the view layer"
    )
    levels = list(library.children)
    check(len(levels) == 3, f"forest: three levels in the library ({len(levels)})")
    counts = [len(level.objects) for level in levels]
    check(counts == [4, 4, 4], f"forest: 2 species x 2 variants per level ({counts})")
    tinted = [m for m in bpy.data.materials if m.name.endswith(" tinted")]
    has_tint = any(
        n.type == "ATTRIBUTE" and n.attribute_type == "INSTANCER"
        for m in tinted
        for n in m.node_tree.nodes
    )
    check(bool(tinted) and has_tint, "forest: materials read the instancer tint")


def check_instances(check, forest: bpy.types.Object, terrain: bpy.types.Object) -> dict:
    """Trees on the ground, inside the terrain, all instanced; returns the counts."""
    points = vertices(forest)
    trees = len(points)
    check(trees == forest["arbrume_forest"] and trees > 50, f"forest: {trees} trees")
    ground = ground_heights(terrain, points)
    sunk = ground - points[:, 2]
    on_ground = np.all(np.isfinite(ground)) and sunk.min() > -0.03 and sunk.max() < 0.3
    worst = points[int(np.nanargmax(sunk))]
    span = f"{sunk.min():.3f} to {sunk.max():.3f} m below the ground, deepest at {worst}"
    check(bool(on_ground), f"forest: every tree stands on the terrain ({span})")
    start = time.perf_counter()
    forest.data.update_tag()
    bpy.context.view_layer.update()
    evaluation = time.perf_counter() - start
    depsgraph = bpy.context.evaluated_depsgraph_get()
    instances = sum(
        1
        for i in depsgraph.object_instances
        if i.is_instance and i.parent and i.parent.original == forest
    )
    check(instances == trees, f"forest: {instances} instances for {trees} trees")
    objects = len(bpy.context.view_layer.objects)
    print(
        f"forest: {trees} trees, {instances} instances, {objects} objects in the view layer, "
        f"evaluated in {evaluation * 1e3:.1f} ms",
        flush=True,
    )
    return {"trees": trees, "instances": instances, "objects": objects}


def render(forest: bpy.types.Object, out: Path) -> None:
    """An EEVEE still of the forest from above at an angle."""
    scene = bpy.context.scene
    bpy.ops.object.camera_add(location=(70.0, -70.0, 55.0), rotation=(0.95, 0.0, 0.785))
    scene.camera = bpy.context.active_object
    bpy.ops.object.light_add(type="SUN", rotation=(0.6, 0.2, 0.8))
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x, scene.render.resolution_y = 480, 320
    scene.render.filepath = str(out / "forest.png")
    start = time.perf_counter()
    bpy.ops.render.render(write_still=True)
    print(f"forest: rendered in {time.perf_counter() - start:.1f} s", flush=True)


def run(ext, check, out: Path) -> None:
    """Every forest step."""
    terrain = add_terrain("Forest Ground")
    result, forest = add_forest(terrain, seed=4)
    check(
        result == {"FINISHED"} and forest is not None, f"forest: the operator finished ({result})"
    )
    if forest is None:
        return
    check_library(ext, check, forest)
    check_instances(check, forest, terrain)
    again = add_forest(terrain, seed=4)[1]
    same = again is not None and np.array_equal(vertices(again), vertices(forest))
    check(same, "forest: the same seed places the same trees")
    other = add_forest(terrain, seed=5)[1]
    check(
        other is not None and not np.array_equal(vertices(other), vertices(forest)),
        "forest: another seed moves them",
    )
    try:
        bpy.ops.arbrume.add_forest(terrain=terrain.name, mix="shrub:-3")
        check(False, "forest: a negative density is refused")
    except RuntimeError:
        check(True, "forest: a negative density is refused")
    for obj in (again, other):
        if obj is not None:
            bpy.data.objects.remove(obj)
    render(forest, out)
