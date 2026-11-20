"""The *Arbrume Forest* operator: a forest placed over a terrain mesh, drawn as instances.

The terrain object is sampled on a regular grid by casting rays straight down onto its
evaluated mesh; samples that miss it take the heights of their neighbours and are
excluded by a mask (``mask_grid``), so no tree stands off the mesh. The grid, the species
mix, the density factor and the seed make the config of ``arbrume.forest``
(``docs/forest.md``), or replace the terrain of a ``forest.toml`` file when one is given.

Each variant of each species is one mesh object per level of detail, in a collection per
level under ``<forest> library``, which is excluded from the view layer. The trees are
the vertices of one mesh object, with the attributes ``arbrume_index`` (which variant),
``arbrume_rotation`` (a quaternion), ``arbrume_scale`` and ``arbrume_tint``, and a
Geometry Nodes modifier instances the variants on them (*Instance on Points*, picking
among the separate children of the collection of a level). The viewport shows the level
``viewport_level`` and renders show level 0, so the scene stays light to draw: a few
objects and one instancer whatever the number of trees. Materials of the forest are
copies of the plant materials whose base colour is multiplied by ``arbrume_tint``, read
from the instancer.
"""

from __future__ import annotations

import time
import tomllib
from pathlib import Path
from typing import ClassVar

import bpy
import numpy as np
from bpy.props import BoolProperty, EnumProperty, FloatProperty, IntProperty, StringProperty
from mathutils.bvhtree import BVHTree

import arbrume

from . import plant
from .logic import forest as logic

TINT = "arbrume_tint"
FOREST_KEY = "arbrume_forest"
UNDO = {"REGISTER", "UNDO"}


def sample_terrain(context, obj: bpy.types.Object, resolution: float):
    """The grid over ``obj`` and the ground height of each sample (``nan`` off the mesh)."""
    depsgraph = context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        mesh.calc_loop_triangles()
        count = len(mesh.vertices)
        local = np.empty(count * 3, dtype=np.float64)
        mesh.vertices.foreach_get("co", local)
        matrix = np.array(obj.matrix_world, dtype=np.float64)
        points = local.reshape(-1, 3) @ matrix[:3, :3].T + matrix[:3, 3]
        triangles = np.empty(len(mesh.loop_triangles) * 3, dtype=np.int32)
        mesh.loop_triangles.foreach_get("vertices", triangles)
    finally:
        evaluated.to_mesh_clear()
    if len(triangles) == 0:
        raise ValueError(f"`{obj.name}` has no faces")
    tree = BVHTree.FromPolygons(points.tolist(), triangles.reshape(-1, 3).tolist())
    low, high = points.min(axis=0), points.max(axis=0)
    grid = logic.grid_over((low[0], low[1]), (high[0], high[1]), resolution)
    xs, ys = grid.blender_xy()
    top = float(high[2]) + 1.0
    heights = np.full(len(xs), np.nan)
    down = (0.0, 0.0, -1.0)
    for index, (x, y) in enumerate(zip(xs, ys, strict=True)):
        hit = tree.ray_cast((float(x), float(y), top), down)
        if hit[0] is not None:
            heights[index] = hit[0][2]
    return grid, heights


def tinted(material: bpy.types.Material) -> bpy.types.Material:
    """A copy of ``material`` whose base colour is multiplied by the instancer's tint."""
    name = f"{material.name} tinted"
    copy = bpy.data.materials.get(name)
    if copy is not None:
        return copy
    copy = material.copy()
    copy.name = name
    tree = copy.node_tree
    bsdf = next((n for n in tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None or not bsdf.inputs["Base Color"].is_linked:
        return copy
    source = bsdf.inputs["Base Color"].links[0].from_socket
    attribute = tree.nodes.new("ShaderNodeAttribute")
    attribute.attribute_type = "INSTANCER"
    attribute.attribute_name = TINT
    attribute.location = (-150, 250)
    mix = tree.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs[0].default_value = 1.0
    mix.location = (-20, 200)
    rgba = [s for s in mix.inputs if s.type == "RGBA"]
    tree.links.new(source, rgba[0])
    tree.links.new(attribute.outputs["Color"], rgba[1])
    result = next(s for s in mix.outputs if s.type == "RGBA")
    tree.links.new(result, bsdf.inputs["Base Color"])
    return copy


def _library(name: str, placed, scene_collection) -> list[bpy.types.Collection]:
    """One collection per level holding every variant, ordered by instance index."""
    library = bpy.data.collections.new(f"{name} library")
    scene_collection.children.link(library)
    depth = max((len(v["levels"]) for s in placed.library for v in s), default=1)
    levels = []
    for level in range(depth):
        collection = bpy.data.collections.new(f"{name} LOD{level}")
        library.children.link(collection)
        levels.append(collection)
    index = 0
    for s, variants in enumerate(placed.library):
        for k, variant in enumerate(variants):
            for level, collection in enumerate(levels):
                tree = variant["levels"][min(level, len(variant["levels"]) - 1)]
                label = f"{name} L{level} {index:05d} {placed.names[s]}_v{k}"
                mesh = plant.mesh_from_tree(label, tree)
                for slot, material in enumerate(mesh.materials):
                    mesh.materials[slot] = tinted(material)
                obj = bpy.data.objects.new(label, mesh)
                collection.objects.link(obj)
            index += 1
    return [library, *levels]


def _points(name: str, placed, counts: list[int]) -> bpy.types.Object:
    """The mesh object whose vertices are the trees, with their attributes."""
    mesh = bpy.data.meshes.new(name)
    n = len(placed.instances)
    mesh.vertices.add(n)
    mesh.vertices.foreach_set("co", logic.blender_points(placed.positions).reshape(-1))
    indices = logic.instance_indices(placed.species, placed.variants, counts)
    mesh.attributes.new("arbrume_index", "INT", "POINT").data.foreach_set("value", indices)
    rotation = mesh.attributes.new("arbrume_rotation", "QUATERNION", "POINT")
    rotation.data.foreach_set("value", logic.blender_quaternions(placed.rotations).reshape(-1))
    scale = mesh.attributes.new("arbrume_scale", "FLOAT", "POINT")
    scale.data.foreach_set("value", np.ascontiguousarray(placed.scales, dtype=np.float32))
    tints = np.ones((n, 4), dtype=np.float32)
    tints[:, :3] = placed.tints
    mesh.attributes.new(TINT, "FLOAT_COLOR", "POINT").data.foreach_set("color", tints.reshape(-1))
    mesh.update()
    return bpy.data.objects.new(name, mesh)


def _named(tree, data_type: str, name: str, location):
    node = tree.nodes.new("GeometryNodeInputNamedAttribute")
    node.data_type = data_type
    node.inputs["Name"].default_value = name
    node.location = location
    return node


def _collection_info(tree, collection, location):
    node = tree.nodes.new("GeometryNodeCollectionInfo")
    node.transform_space = "ORIGINAL"
    node.inputs["Collection"].default_value = collection
    node.inputs["Separate Children"].default_value = True
    node.inputs["Reset Children"].default_value = True
    node.location = location
    return node


def _node_group(name: str, viewport, render) -> bpy.types.NodeTree:
    """Instances the children of ``viewport`` (in the viewport) or ``render`` (in renders)
    on the points, picked by ``arbrume_index``, turned and scaled."""
    tree = bpy.data.node_groups.new(f"{name} instancer", "GeometryNodeTree")
    tree.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
    tree.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
    nodes, links = tree.nodes, tree.links
    group_in = nodes.new("NodeGroupInput")
    group_in.location = (-600, 0)
    group_out = nodes.new("NodeGroupOutput")
    group_out.location = (400, 0)
    switch = nodes.new("GeometryNodeSwitch")
    switch.input_type = "GEOMETRY"
    switch.location = (-200, -200)
    is_viewport = nodes.new("GeometryNodeIsViewport")
    is_viewport.location = (-400, -100)
    links.new(is_viewport.outputs[0], switch.inputs["Switch"])
    links.new(_collection_info(tree, viewport, (-400, -250)).outputs[0], switch.inputs["True"])
    links.new(_collection_info(tree, render, (-400, -450)).outputs[0], switch.inputs["False"])
    instance = nodes.new("GeometryNodeInstanceOnPoints")
    instance.location = (100, 0)
    instance.inputs["Pick Instance"].default_value = True
    links.new(group_in.outputs[0], instance.inputs["Points"])
    links.new(switch.outputs[0], instance.inputs["Instance"])
    links.new(
        _named(tree, "INT", "arbrume_index", (-200, 150)).outputs["Attribute"],
        instance.inputs["Instance Index"],
    )
    links.new(
        _named(tree, "QUATERNION", "arbrume_rotation", (-200, 50)).outputs["Attribute"],
        instance.inputs["Rotation"],
    )
    links.new(
        _named(tree, "FLOAT", "arbrume_scale", (-200, -50)).outputs["Attribute"],
        instance.inputs["Scale"],
    )
    links.new(instance.outputs[0], group_out.inputs[0])
    return tree


def build_forest(context, placed, name: str, viewport_level: int) -> bpy.types.Object:
    """Adds the library and the instancer of ``placed`` (an ``arbrume.Forest`` with its
    library grown) to the scene; returns the instancer object."""
    if not placed.library:
        raise ValueError("the forest has no variant library")
    scene_collection = context.scene.collection
    library, *levels = _library(name, placed, scene_collection)
    for child in context.view_layer.layer_collection.children:
        if child.collection == library:
            child.exclude = True
    counts = [len(variants) for variants in placed.library]
    obj = _points(name, placed, counts)
    scene_collection.objects.link(obj)
    viewport = levels[min(max(viewport_level, 0), len(levels) - 1)]
    modifier = obj.modifiers.new("Arbrume Forest", "NODES")
    modifier.node_group = _node_group(name, viewport, levels[0])
    obj[FOREST_KEY] = len(placed.instances)
    obj["arbrume_species"] = list(placed.names)
    return obj


def instance_count(context, obj: bpy.types.Object) -> int:
    """Instances the evaluated instancer ``obj`` makes."""
    depsgraph = context.evaluated_depsgraph_get()
    return sum(
        1
        for i in depsgraph.object_instances
        if i.is_instance and i.parent and i.parent.original == obj
    )


class ARBRUME_OT_add_forest(bpy.types.Operator):
    """Place a forest over a terrain mesh and draw it as instances of a few variants."""

    bl_idname = "arbrume.add_forest"
    bl_label = "Arbrume Forest"
    bl_options: ClassVar[set[str]] = UNDO

    terrain: StringProperty(name="Terrain", description="Mesh object the trees stand on")
    mix: StringProperty(
        name="Species Mix",
        description="Species and trees per hectare: `preset:density, preset:density`",
        default=logic.DEFAULT_MIX,
    )
    density: FloatProperty(
        name="Density", description="Factor on every density", default=1.0, min=0.0, soft_max=4.0
    )
    seed: IntProperty(name="Seed", default=0, min=0)
    variants: IntProperty(
        name="Variants", description="Variants per species", default=3, min=1, max=16
    )
    lods: IntProperty(name="Mesh Levels", default=2, min=1, max=5)
    billboard: EnumProperty(
        name="Billboard",
        items=[("cloud", "Cloud", ""), ("cross", "Crossed Quads", ""), ("none", "None", "")],
        default="cloud",
    )
    textures: BoolProperty(name="Textures", default=True)
    resolution: FloatProperty(
        name="Terrain Resolution",
        description="Sample spacing",
        default=2.0,
        min=0.1,
        subtype="DISTANCE",
    )
    viewport_level: IntProperty(
        name="Viewport Level",
        description="Level of detail drawn in the viewport (renders use 0)",
        default=99,
        min=0,
    )
    config: StringProperty(
        name="Config",
        description="Optional forest.toml; its terrain is replaced by the mesh",
        subtype="FILE_PATH",
    )

    def draw(self, context) -> None:
        layout = self.layout
        layout.prop_search(self, "terrain", context.scene, "objects")
        for name in (
            "mix",
            "density",
            "seed",
            "variants",
            "lods",
            "billboard",
            "textures",
            "resolution",
            "viewport_level",
            "config",
        ):
            layout.prop(self, name)

    def invoke(self, context, event):
        active = context.active_object
        if not self.terrain and active is not None and active.type == "MESH":
            self.terrain = active.name
        return self.execute(context)

    def _config(self, grid, heights) -> tuple[dict, Path | None]:
        options = {
            "seed": self.seed,
            "density": self.density,
            "variants": self.variants,
            "lods": self.lods,
            "billboard": self.billboard,
            "textures": self.textures,
        }
        config = logic.forest_config(grid, heights, logic.parse_mix(self.mix), options)
        if not self.config:
            return config, None
        path = Path(bpy.path.abspath(self.config))
        with path.open("rb") as file:
            loaded = tomllib.load(file)
        loaded["terrain"] = config["terrain"]
        loaded.setdefault("placement", {}).update(config.get("placement", {}))
        loaded.setdefault("seed", self.seed)
        return loaded, path.parent

    def execute(self, context):
        obj = context.scene.objects.get(self.terrain)
        if obj is None or obj.type != "MESH":
            self.report({"ERROR"}, "Pick a terrain mesh object")
            return {"CANCELLED"}
        start = time.perf_counter()
        try:
            grid, heights = sample_terrain(context, obj, self.resolution)
            config, base = self._config(grid, heights)
            placed = arbrume.forest(config, base=base)
        except (ValueError, OSError, tomllib.TOMLDecodeError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        forest = build_forest(context, placed, f"{obj.name} forest", self.viewport_level)
        for other in context.selected_objects:
            other.select_set(False)
        forest.select_set(True)
        context.view_layer.objects.active = forest
        seconds = time.perf_counter() - start
        self.report(
            {"INFO"},
            f"{len(placed.instances)} trees of {len(placed.names)} species in {seconds:.1f} s",
        )
        return {"FINISHED"}


def menu_add(self, _context) -> None:
    """Entry of the Add > Mesh menu."""
    self.layout.operator(ARBRUME_OT_add_forest.bl_idname, icon="OUTLINER_OB_FORCE_FIELD")


CLASSES = (ARBRUME_OT_add_forest,)
