"""Steps of the in-Blender test for aquatic plants and underwater forests.

Imported by ``run_in_blender.py`` inside Blender; every step takes the extension module and
the ``check`` function of the runner. A kelp stays under its water surface, whether the
surface comes from its species, from a height or from a plane; a current bends it
downstream; a land plant ignores the water; the forest operator puts aquatic species
under a water plane and land species above it.
"""

from __future__ import annotations

import bpy
import numpy as np
from forest_steps import vertices
from surroundings_steps import new_plant, world_vertices

#: Tolerance on heights at the surface, in meters: floating blades are meshed with their
#: random tilt, a few centimeters at most.
ON_SURFACE = 0.12


def add_plane(name: str, height: float, size: float = 20.0) -> bpy.types.Object:
    """A horizontal plane at ``height``."""
    bpy.ops.mesh.primitive_plane_add(size=size, location=(0.0, 0.0, height))
    plane = bpy.context.active_object
    plane.name = name
    return plane


def test_surface(ext, check) -> None:
    """The kelp stays under the surface of its species, of a height and of a plane."""
    plant = new_plant("bull_kelp_like", seed=2, age=12, quality=0.3)
    settings = plant.arbrume
    top = world_vertices(plant)[:, 2].max()
    check(top <= 10.0 + ON_SURFACE, f"water: the species depth holds the kelp: {top:.2f} m")
    settings.water_mode = "HEIGHT"
    settings.water_height = 4.0
    top = world_vertices(plant)[:, 2].max()
    check(3.6 < top <= 4.0 + ON_SURFACE, f"water: a height of 4 m holds the kelp: {top:.2f} m")
    plane = add_plane("water surface", 6.0)
    settings.water_object = plane
    settings.water_mode = "OBJECT"
    top = world_vertices(plant)[:, 2].max()
    check(5.6 < top <= 6.0 + ON_SURFACE, f"water: a plane at 6 m holds the kelp: {top:.2f} m")
    still = world_vertices(plant)[:, 0].mean()
    settings.water_current = (0.4, 0.0)
    drift = world_vertices(plant)[:, 0].mean() - still
    check(drift > 0.3, f"water: a current along X bends the kelp along X: {drift:.2f} m")
    shrub = new_plant("shrub", seed=1, age=4, quality=0.3)
    dry = world_vertices(shrub)
    shrub.arbrume.water_mode = "HEIGHT"
    shrub.arbrume.water_height = 0.5
    check(np.array_equal(dry, world_vertices(shrub)), "water: a land plant ignores the water")
    for obj in (plant, plane, shrub):
        bpy.data.objects.remove(obj)


def add_basin(name: str) -> bpy.types.Object:
    """A grid 30 m wide sloping from 1 m above the water to 7 m under it, along X."""
    bpy.ops.mesh.primitive_grid_add(x_subdivisions=30, y_subdivisions=30, size=30.0)
    obj = bpy.context.active_object
    obj.name = name
    co = vertices(obj)
    co[:, 2] = 1.0 - 8.0 * (co[:, 0] + 15.0) / 30.0
    obj.data.vertices.foreach_set("co", co.reshape(-1))
    obj.data.update()
    return obj


def test_forest(check) -> None:
    """Aquatic species stand under the water plane, land species above it."""
    basin = add_basin("Seabed")
    plane = add_plane("sea surface", 0.0, 40.0)
    before = set(bpy.data.objects)
    result = bpy.ops.arbrume.add_forest(
        terrain=basin.name,
        mix="bull_kelp_like:400, rosette_herb:3000",
        seed=1,
        variants=1,
        lods=1,
        billboard="none",
        textures=False,
        resolution=1.0,
        water="OBJECT",
        water_object=plane.name,
    )
    new = [o for o in bpy.data.objects if o not in before and o.modifiers]
    check(result == {"FINISHED"} and len(new) == 1, f"water: the forest operator ran ({result})")
    if len(new) != 1:
        return
    forest = new[0]
    points = vertices(forest)
    index = np.zeros(len(points), dtype=np.int32)
    forest.data.attributes["arbrume_index"].data.foreach_get("value", index)
    kelp, herbs = points[index == 0], points[index == 1]
    check(len(kelp) > 0 and len(herbs) > 0, f"water: {len(kelp)} kelp, {len(herbs)} herbs")
    check(bool((kelp[:, 2] < 0.0).all()), "water: every kelp stands under the water")
    check(bool((herbs[:, 2] > 0.0).all()), "water: every herb stands above the water")
    for obj in (forest, basin, plane):
        bpy.data.objects.remove(obj)


def run(ext, check) -> None:
    """Every step."""
    test_surface(ext, check)
    test_forest(check)
