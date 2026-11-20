"""Steps of the in-Blender test for root systems.

Imported by ``run_in_blender.py`` inside Blender; every step takes the extension module and
the ``check`` function of the runner. A rooted preset grows its roots below the ground,
the *Roots* setting turns them off and on, the parameters of the ``[roots]`` layer edit
them, a sphere under the ground is avoided by the roots of a parametric species, and one
Cycles still shows a root system cut away from its soil.
"""

from __future__ import annotations

from pathlib import Path

import bpy
import numpy as np
from mathutils import Vector
from surroundings_steps import add_sphere, generated_tree, new_plant, world_vertices

ROCK_CENTER = (0.9, 0.0, -1.0)
ROCK_RADIUS = 0.6


def root_nodes(tree) -> np.ndarray:
    """Skeleton nodes under the ground (world space, Z up), the roots of a plant."""
    nodes = tree.skeleton.positions
    live = ~tree.skeleton.shed.astype(bool)
    world = np.stack([nodes[:, 0], -nodes[:, 2], nodes[:, 1]], axis=1)
    return world[live & (world[:, 2] < -1e-3)]


def test_modes(ext, check) -> bpy.types.Object:
    """The roots of a rooted preset, without them, and added to a species without."""
    plant = new_plant("maple_like_rooted", seed=2, age=14)
    settings = plant.arbrume
    check(settings.roots == "species", "roots follow the species by default")
    sections = [section.name for section in ext.state.STATE.sections]
    check("roots" in sections, f"a Roots panel from the schema: {sections}")
    low = world_vertices(plant)[:, 2].min()
    check(low < -0.5, f"the rooted preset has roots under the ground: {low:.2f}")
    triangles = len(plant.data.polygons)
    check(triangles == generated_tree(ext, plant).stats["triangles"], "rooted counts")
    settings.roots = "off"
    low = world_vertices(plant)[:, 2].min()
    check(low > -0.05, f"no roots when off: lowest vertex {low:.2f}")
    check(len(plant.data.polygons) < triangles, "fewer triangles without roots")
    settings.roots = "species"
    settings.preset = "broadleaf"
    check(world_vertices(plant)[:, 2].min() > -0.05, "broadleaf has no roots")
    settings.roots = "on"
    check(world_vertices(plant)[:, 2].min() < -0.5, "roots added to broadleaf")
    before = len(plant.data.polygons)
    settings.params.roots__primaries = 3
    check(len(plant.data.polygons) < before, "fewer primary roots, fewer triangles")
    check(
        ext.logic.values.load_overrides(settings.overrides).get("roots.primaries") == 3,
        "the parameter is an override",
    )
    return plant


def test_rock(ext, check, plant) -> None:
    """A sphere under the ground blocks the roots of a parametric species."""
    settings = plant.arbrume
    settings.preset = "oak_like_rooted"
    settings.roots = "species"
    settings.overrides = "{}"
    center, radius = np.array(ROCK_CENTER), ROCK_RADIUS
    inside = np.linalg.norm(root_nodes(generated_tree(ext, plant)) - center, axis=1) < radius
    check(inside.any(), f"without the rock {int(inside.sum())} root nodes are in it")
    rock = add_sphere("rock", ROCK_CENTER, ROCK_RADIUS)
    item = settings.obstacles.add()
    item.obj = rock
    around, _ = ext.plant.surroundings_of(plant)
    check(around is not None, "a rooted parametric species reads its obstacles")
    nodes = root_nodes(generated_tree(ext, plant))
    inside = np.linalg.norm(nodes - center, axis=1) < radius - 0.05
    check(not inside.any(), f"with the rock no root node inside it ({int(inside.sum())})")
    check(len(nodes) > 50, f"the roots grow around the rock: {len(nodes)} nodes")
    settings.obstacles.remove(0)
    bpy.data.objects.remove(rock)


def run(ext, check) -> None:
    """Every step of this module."""
    plant = test_modes(ext, check)
    test_rock(ext, check, plant)
    bpy.data.objects.remove(plant)


def render_still(ext, out: Path) -> None:
    """A Cycles still of a rooted spruce next to a stone, the ground left out."""
    scene = bpy.context.scene
    engine = scene.render.engine
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 48
    scene.cycles.use_denoising = False
    for name in ("ground",):
        if name in bpy.data.objects:
            bpy.data.objects[name].hide_render = True
    for obj in scene.objects:
        if obj.type == "MESH" and obj.name != "ground":
            obj.hide_render = True
    plant = new_plant("spruce_like_rooted", seed=3, age=30, quality=0.8)
    plant.arbrume.textures = True
    rock = add_sphere("stone", (1.6, 0.0, -0.9), 0.55)
    stone = bpy.data.materials.new("stone")
    if not stone.use_nodes:
        stone.use_nodes = True
    stone.node_tree.nodes["Principled BSDF"].inputs[0].default_value = (0.3, 0.29, 0.27, 1)
    rock.data.materials.append(stone)
    plant.arbrume.obstacles.add().obj = rock
    plant.arbrume.seed = 3
    camera = scene.camera
    target = Vector((0.0, 0.0, -0.6))
    camera.location = target + Vector((0.0, -11.0, 0.6))
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out / "roots_cycles.png")
    bpy.ops.render.render(write_still=True)
    scene.render.engine = engine
    plant.hide_render = rock.hide_render = True
