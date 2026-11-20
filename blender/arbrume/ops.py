"""Operators: adding, regenerating, varying, scattering, LODs, growth frames, obstacles,
resets and species files."""

import math
import random
from pathlib import Path
from typing import ClassVar

import bpy
from bpy.props import BoolProperty, EnumProperty, FloatProperty, IntProperty, StringProperty
from bpy_extras.io_utils import ExportHelper, ImportHelper

import arbrume

from . import plant, props, surroundings
from .logic import frames, toml_out
from .logic.species import Source, full_species
from .logic.values import (
    CurvePoints,
    dump_overrides,
    entry_at,
    entry_display_value,
    entry_points,
    load_overrides,
    points_entry,
    without_override,
)
from .state import STATE, suspended

UNDO = {"REGISTER", "UNDO"}
SETTINGS_FIELDS = (
    "preset_name",
    "custom_toml",
    "custom_name",
    "seed",
    "age",
    "scale",
    "quality",
    "textures",
    "junctions",
    "overrides",
    "guides",
    "live_update",
    "envelope",
    "envelope_ground",
    "voxel_size",
)


def active_plant(context) -> bpy.types.Object | None:
    """The active object if it is an Arbrume plant."""
    obj = getattr(context, "active_object", None)
    return obj if plant.is_plant(obj) else None


def selected_plants(context) -> list[bpy.types.Object]:
    """Selected plants, or the active plant alone when none is selected."""
    chosen = [o for o in getattr(context, "selected_objects", []) if plant.is_plant(o)]
    active = active_plant(context)
    return chosen or ([active] if active else [])


def copy_settings(source, target, **changes) -> None:
    """Copies the plant settings of ``source`` to ``target``, then applies ``changes``."""
    with suspended():
        target.is_plant = True
        for name in SETTINGS_FIELDS:
            setattr(target, name, changes.get(name, getattr(source, name)))
        target.obstacles.clear()
        for item in source.obstacles:
            copy = target.obstacles.add()
            copy.obj, copy.blocks, copy.shade = item.obj, item.blocks, item.shade
            copy.role = item.role


def new_plant_object(context, name: str, collection=None) -> bpy.types.Object:
    """An empty plant object at the 3D cursor, linked to ``collection`` or the active one."""
    mesh = bpy.data.meshes.new(name)
    obj = bpy.data.objects.new(name, mesh)
    (collection or context.collection).objects.link(obj)
    obj.location = context.scene.cursor.location
    with suspended():
        obj.arbrume.is_plant = True
    return obj


def discard(obj: bpy.types.Object) -> None:
    """Removes an object and its mesh if nothing else uses the mesh."""
    mesh = obj.data
    bpy.data.objects.remove(obj)
    if mesh is not None and mesh.users == 0:
        bpy.data.meshes.remove(mesh)


def make_active(context, obj: bpy.types.Object) -> None:
    """Selects ``obj`` alone and makes it active."""
    for other in context.selected_objects:
        other.select_set(False)
    obj.select_set(True)
    context.view_layer.objects.active = obj


def species_age(source: Source) -> int | None:
    """Oldest age suggested by a species (``variation.age``), if it gives one."""
    ages = STATE.cache.base(source).get("variation", {}).get("age")
    return int(ages[1]) if isinstance(ages, list) and len(ages) == 2 else None


def new_seed() -> int:
    """A random plant seed."""
    return random.randint(0, props.MAX_SEED)


class ARBRUME_OT_add_plant(bpy.types.Operator):
    """Add a plant generated from a species preset"""

    bl_idname = "arbrume.add_plant"
    bl_label = "Arbrume Plant"
    bl_options: ClassVar[set[str]] = UNDO

    preset: EnumProperty(name="Species", items=props.preset_items)
    seed: IntProperty(name="Seed", min=0, max=props.MAX_SEED)
    use_species_age: BoolProperty(
        name="Species age", description="Use the oldest age suggested by the species", default=True
    )
    age: IntProperty(name="Age", default=20, min=0, max=1000, soft_max=100)
    scale: FloatProperty(name="Scale", default=1.0, min=0.01, max=100.0, soft_max=3.0)
    quality: FloatProperty(name="Quality", default=1.0, min=0.01, max=4.0, soft_max=1.0)

    @classmethod
    def poll(cls, context):
        return context.mode == "OBJECT" and bool(STATE.presets)

    def invoke(self, context, _event):
        if not self.properties.is_property_set("quality"):
            self.quality = plant.preferences().default_quality
        return self.execute(context)

    def draw(self, _context):
        layout = self.layout
        layout.use_property_split = True
        for name in ("preset", "seed", "use_species_age"):
            layout.prop(self, name)
        row = layout.row()
        row.enabled = not self.use_species_age
        row.prop(self, "age")
        layout.prop(self, "scale")
        layout.prop(self, "quality")

    def execute(self, context):
        source = Source(preset=self.preset)
        age = species_age(source) if self.use_species_age else None
        obj = new_plant_object(context, props.species_display_name(self.preset))
        with suspended():
            settings = obj.arbrume
            settings.preset_name = self.preset
            settings.seed, settings.scale, settings.quality = self.seed, self.scale, self.quality
            settings.age = self.age if age is None else age
        error = plant.regenerate(obj)
        if error:
            discard(obj)
            self.report({"ERROR"}, error)
            return {"CANCELLED"}
        make_active(context, obj)
        return {"FINISHED"}


class ARBRUME_OT_regenerate(bpy.types.Operator):
    """Generate the selected plants again from their settings"""

    bl_idname = "arbrume.regenerate"
    bl_label = "Regenerate"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        return bool(selected_plants(context))

    def execute(self, context):
        errors = [e for o in selected_plants(context) if (e := plant.regenerate(o))]
        if errors:
            self.report({"ERROR"}, errors[0])
            return {"CANCELLED"}
        return {"FINISHED"}


class ARBRUME_OT_randomize_seed(bpy.types.Operator):
    """Give the selected plants new random seeds"""

    bl_idname = "arbrume.randomize_seed"
    bl_label = "Randomize Seed"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        return bool(selected_plants(context))

    def execute(self, context):
        for obj in selected_plants(context):
            with suspended():
                obj.arbrume.seed = new_seed()
            error = plant.regenerate(obj)
            if error:
                self.report({"ERROR"}, error)
                return {"CANCELLED"}
        return {"FINISHED"}


class ARBRUME_OT_duplicate_new_seed(bpy.types.Operator):
    """Duplicate the active plant with a new random seed"""

    bl_idname = "arbrume.duplicate_new_seed"
    bl_label = "Duplicate with New Seed"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        return context.mode == "OBJECT" and active_plant(context) is not None

    def execute(self, context):
        source = active_plant(context)
        obj = source.copy()
        obj.data = bpy.data.meshes.new(source.data.name)
        for material in source.data.materials:
            obj.data.materials.append(material)
        for collection in source.users_collection:
            collection.objects.link(obj)
        copy_settings(source.arbrume, obj.arbrume, seed=new_seed())
        error = plant.regenerate(obj)
        if error:
            discard(obj)
            self.report({"ERROR"}, error)
            return {"CANCELLED"}
        make_active(context, obj)
        return {"FINISHED"}


def scatter_positions(context, source, mode: str, count: int, spacing: float) -> list:
    """Positions of the scattered plants: a grid around the source, or selected objects."""
    if mode == "SELECTED":
        return [o.matrix_world.translation.copy() for o in context.selected_objects if o != source]
    columns = max(1, math.ceil(math.sqrt(count)))
    origin = source.matrix_world.translation
    return [
        origin.copy() + type(origin)(((i % columns + 1) * spacing, (i // columns) * spacing, 0.0))
        for i in range(count)
    ]


def plant_width(tree) -> float:
    """Horizontal extent of a generated tree (Arbrume X and Z), in meters."""
    bounds = tree.stats.get("bounds", {})
    low, high = bounds.get("min", [0, 0, 0]), bounds.get("max", [0, 0, 0])
    return max(high[0] - low[0], high[2] - low[2], 0.1)


class ARBRUME_OT_scatter(bpy.types.Operator):
    """Create plants with different seeds in a grid or at the selected objects"""

    bl_idname = "arbrume.scatter"
    bl_label = "Scatter Variations"
    bl_options: ClassVar[set[str]] = UNDO

    mode: EnumProperty(
        name="Placement",
        items=[
            ("GRID", "Grid", "In a grid next to the active plant"),
            ("SELECTED", "Selected Objects", "At the origins of the other selected objects"),
        ],
    )
    count: IntProperty(name="Count", default=9, min=1, max=10000, soft_max=100)
    variations: IntProperty(
        name="Variations",
        description="Distinct seeds; plants with the same seed share one mesh. 0: all differ",
        default=0,
        min=0,
        max=10000,
        soft_max=100,
    )
    spacing: FloatProperty(
        name="Spacing", description="Grid spacing, 0 for the plant width", default=0.0, min=0.0
    )
    random_rotation: BoolProperty(name="Random Rotation", default=True)

    @classmethod
    def poll(cls, context):
        return context.mode == "OBJECT" and active_plant(context) is not None

    def generate(self, source_settings, seeds: list[int]) -> list:
        source = plant.source_of(source_settings)
        args = plant.generation_args(source_settings)
        jobs = [
            {
                "species": source.argument,
                "seed": seed,
                "age": args["age"],
                "overrides": load_overrides(source_settings.overrides),
            }
            for seed in seeds
        ]
        threads = plant.preferences().threads or None
        return arbrume.generate_many(
            jobs,
            threads=threads,
            scale=args["scale"],
            quality=args["quality"],
            textures=args["textures"],
            junctions=args["junctions"],
            water=args["water"],
        )

    def execute(self, context):
        source = active_plant(context)
        settings = source.arbrume
        count = self.count
        if self.mode == "SELECTED":
            count = len([o for o in context.selected_objects if o != source])
            if count == 0:
                self.report({"ERROR"}, "Select the objects to place plants at")
                return {"CANCELLED"}
        kinds = min(count, self.variations or count)
        seeds = [
            arbrume.plant_seed(settings.seed, i + 1) % (props.MAX_SEED + 1) for i in range(kinds)
        ]
        try:
            trees = self.generate(settings, seeds)
        except (ValueError, TypeError, OSError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        spacing = self.spacing or 1.1 * max(plant_width(t) for t in trees) * settings.scale
        positions = scatter_positions(context, source, self.mode, count, spacing)
        collection = bpy.data.collections.new(f"{source.name} scatter")
        context.scene.collection.children.link(collection)
        meshes = [
            plant.mesh_from_tree(f"{source.name} {seed}", t)
            for seed, t in zip(seeds, trees, strict=True)
        ]
        rng = random.Random(settings.seed)
        for index, position in enumerate(positions):
            obj = bpy.data.objects.new(f"{source.name} {index + 1}", meshes[index % kinds])
            collection.objects.link(obj)
            obj.location = position
            obj.rotation_euler = source.rotation_euler
            if self.random_rotation:
                obj.rotation_euler.z = rng.uniform(0.0, 2.0 * math.pi)
            copy_settings(settings, obj.arbrume, seed=seeds[index % kinds])
        self.report({"INFO"}, f"{count} plants, {kinds} meshes, in {collection.name}")
        return {"FINISHED"}


class ARBRUME_OT_make_lods(bpy.types.Operator):
    """Create copies of the active plant at decreasing quality in a new collection"""

    bl_idname = "arbrume.make_lods"
    bl_label = "Create LODs"
    bl_options: ClassVar[set[str]] = UNDO

    levels: IntProperty(name="Levels", default=3, min=1, max=8)
    factor: FloatProperty(
        name="Quality Factor",
        description="Quality of each level relative to the previous one",
        default=0.5,
        min=0.05,
        max=0.95,
    )
    hide_lower: BoolProperty(name="Hide Lower Levels", default=True)

    @classmethod
    def poll(cls, context):
        return context.mode == "OBJECT" and active_plant(context) is not None

    def execute(self, context):
        source = active_plant(context)
        collection = bpy.data.collections.new(f"{source.name} LODs")
        context.scene.collection.children.link(collection)
        for level in range(self.levels):
            quality = max(0.01, source.arbrume.quality * self.factor**level)
            obj = new_plant_object(context, f"{source.name}_LOD{level}", collection)
            obj.matrix_world = source.matrix_world
            copy_settings(source.arbrume, obj.arbrume, quality=quality)
            error = plant.regenerate(obj)
            if error:
                self.report({"ERROR"}, error)
                return {"CANCELLED"}
            if level > 0 and self.hide_lower:
                obj.hide_set(True)
        self.report({"INFO"}, f"{self.levels} levels in {collection.name}")
        return {"FINISHED"}


class ARBRUME_OT_reset_param(bpy.types.Operator):
    """Reset a species parameter to the value of the species, or all of them"""

    bl_idname = "arbrume.reset_param"
    bl_label = "Reset to Species"
    bl_options: ClassVar[set[str]] = UNDO

    path: StringProperty(name="Parameter", description="Dotted path; empty resets all")

    @classmethod
    def poll(cls, context):
        obj = active_plant(context)
        return obj is not None and plant.is_editable(obj)

    def execute(self, context):
        settings = active_plant(context).arbrume
        overrides = load_overrides(settings.overrides)
        overrides = without_override(overrides, self.path) if self.path else {}
        settings.overrides = dump_overrides(overrides)
        return {"FINISHED"}


class ARBRUME_OT_curve_mode(bpy.types.Operator):
    """Edit a curve parameter of the selected order as a curve, or flatten it"""

    bl_idname = "arbrume.curve_mode"
    bl_label = "Curve Mode"
    bl_options: ClassVar[set[str]] = UNDO

    path: StringProperty(name="Parameter")
    to_curve: BoolProperty(name="To Curve", default=True)

    @classmethod
    def poll(cls, context):
        obj = active_plant(context)
        return obj is not None and plant.is_editable(obj)

    def execute(self, context):
        spec = STATE.specs.get(self.path)
        if spec is None:
            self.report({"ERROR"}, f"Unknown parameter {self.path}")
            return {"CANCELLED"}
        settings = active_plant(context).arbrume
        order = props.curve_order_of(settings, spec)
        entry = entry_at(props.current_value(settings, spec), order)
        if self.to_curve:
            points = entry_points(entry) or CurvePoints.flat(entry_display_value(entry))
            props.write_curve_entry(settings, spec, points_entry(points))
        else:
            props.write_curve_entry(settings, spec, entry_display_value(entry))
        return {"FINISHED"}


class ARBRUME_OT_export_species(bpy.types.Operator, ExportHelper):
    """Save the species of the active plant, with its changes, as a species file"""

    bl_idname = "arbrume.export_species"
    bl_label = "Save Species File"
    bl_options: ClassVar[set[str]] = {"REGISTER"}

    filename_ext = ".toml"
    filter_glob: StringProperty(default="*.toml", options={"HIDDEN"})
    species_name: StringProperty(name="Species Name", description="Name written in the file")

    @classmethod
    def poll(cls, context):
        return active_plant(context) is not None

    def invoke(self, context, event):
        obj = active_plant(context)
        if not self.species_name:
            self.species_name = obj.name
        if not self.filepath:
            self.filepath = bpy.path.clean_name(obj.name).lower() + self.filename_ext
        return ExportHelper.invoke(self, context, event)

    def execute(self, context):
        settings = active_plant(context).arbrume
        overrides = load_overrides(settings.overrides)
        try:
            species = full_species(plant.source_of(settings), overrides)
        except (ValueError, TypeError, OSError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        header = (
            f"Species file written by Arbrume {arbrume.__version__} from "
            f"{plant.species_label(settings)} with {len(overrides)} changed parameters."
        )
        text = toml_out.export_text(species, self.species_name or "Custom species", header)
        try:
            Path(self.filepath).write_text(text, encoding="utf-8")
        except OSError as error:
            self.report({"ERROR"}, f"Cannot write {self.filepath}: {error}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Saved {self.filepath}")
        return {"FINISHED"}


class ARBRUME_OT_import_species(bpy.types.Operator, ImportHelper):
    """Load a species file into the active plant, or as a new plant"""

    bl_idname = "arbrume.import_species"
    bl_label = "Load Species File"
    bl_options: ClassVar[set[str]] = UNDO

    filename_ext = ".toml"
    filter_glob: StringProperty(default="*.toml", options={"HIDDEN"})
    new_plant: BoolProperty(
        name="New Plant", description="Add a new plant instead of changing the active one"
    )

    @classmethod
    def poll(cls, context):
        return context.mode == "OBJECT"

    def execute(self, context):
        try:
            text = Path(self.filepath).read_text(encoding="utf-8")
            name = full_species(Source(toml=text)).get("name") or Path(self.filepath).stem
        except (ValueError, TypeError, OSError, UnicodeDecodeError) as error:
            self.report({"ERROR"}, f"Cannot load {self.filepath}: {error}")
            return {"CANCELLED"}
        obj = active_plant(context)
        created = self.new_plant or obj is None or not plant.is_editable(obj)
        if created:
            obj = new_plant_object(context, name)
        with suspended():
            obj.arbrume.custom_toml = text
            obj.arbrume.custom_name = name
            obj.arbrume.overrides = "{}"
            if created:
                obj.arbrume.age = species_age(Source(toml=text)) or obj.arbrume.age
        error = plant.regenerate(obj)
        if error:
            if created:
                discard(obj)
            self.report({"ERROR"}, error)
            return {"CANCELLED"}
        make_active(context, obj)
        return {"FINISHED"}


class ARBRUME_OT_add_obstacle(bpy.types.Operator):
    """Add the selected mesh objects as obstacles of the active plant, or an empty slot"""

    bl_idname = "arbrume.add_obstacle"
    bl_label = "Add Obstacle"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        return active_plant(context) is not None

    def execute(self, context):
        obj = active_plant(context)
        settings = obj.arbrume
        known = {item.obj for item in settings.obstacles} | {settings.envelope}
        chosen = [
            o
            for o in context.selected_objects
            if o != obj and surroundings.is_volume(o) and o not in known
        ]
        with suspended():
            for other in chosen or [None]:
                settings.obstacles.add().obj = other
        if chosen:
            plant.regenerate_if_live(obj)
        return {"FINISHED"}


class ARBRUME_OT_remove_obstacle(bpy.types.Operator):
    """Remove one obstacle of the active plant"""

    bl_idname = "arbrume.remove_obstacle"
    bl_label = "Remove Obstacle"
    bl_options: ClassVar[set[str]] = UNDO

    index: IntProperty(name="Index", min=0)

    @classmethod
    def poll(cls, context):
        return active_plant(context) is not None

    def execute(self, context):
        obj = active_plant(context)
        if self.index >= len(obj.arbrume.obstacles):
            return {"CANCELLED"}
        with suspended():
            obj.arbrume.obstacles.remove(self.index)
        plant.regenerate_if_live(obj)
        return {"FINISHED"}


def key_visibility(obj: bpy.types.Object, keys: list[tuple[int, bool]]) -> None:
    """Keyframes the viewport and render visibility of ``obj``."""
    for frame, hidden in keys:
        obj.hide_viewport = obj.hide_render = hidden
        obj.keyframe_insert("hide_viewport", frame=frame)
        obj.keyframe_insert("hide_render", frame=frame)


class ARBRUME_OT_growth_frames(bpy.types.Operator):
    """Create one object per growth stage of the active plant in a new collection, each
    shown in turn on the timeline"""

    bl_idname = "arbrume.growth_frames"
    bl_label = "Growth Frames"
    bl_options: ClassVar[set[str]] = UNDO

    stages: IntProperty(
        name="Stages",
        description="Growth stages, spread evenly over the age of the plant",
        default=10,
        min=1,
        max=500,
        soft_max=60,
    )
    step: IntProperty(
        name="Frames per Stage", description="Timeline frames each stage is shown", default=5, min=1
    )
    hide_source: BoolProperty(name="Hide Plant", description="Hide the plant itself", default=True)

    @classmethod
    def poll(cls, context):
        return context.mode == "OBJECT" and active_plant(context) is not None

    def grow(self, source) -> list:
        """The trees of the growth stages of the plant ``source``."""
        settings = source.arbrume
        around, _ = plant.surroundings_of(source)
        return arbrume.growth_frames(
            plant.source_of(settings).argument,
            frames=self.stages,
            overrides=load_overrides(settings.overrides) or None,
            surroundings=around,
            **plant.generation_args(settings),
        )

    def execute(self, context):
        source = active_plant(context)
        try:
            trees = self.grow(source)
        except (ValueError, TypeError, OSError, RuntimeError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        if not trees:
            self.report({"ERROR"}, "The plant has no growth cycle (age 0)")
            return {"CANCELLED"}
        scene = context.scene
        collection = bpy.data.collections.new(f"{source.name} growth")
        scene.collection.children.link(collection)
        start = scene.frame_start
        schedule = frames.visibility_keys(len(trees), start, self.step)
        for index, (tree, keys) in enumerate(zip(trees, schedule, strict=True)):
            name = f"{source.name} growth {index + 1:02d}"
            obj = bpy.data.objects.new(name, plant.mesh_from_tree(name, tree))
            collection.objects.link(obj)
            obj.matrix_world = source.matrix_world
            obj["arbrume_growth_cycle"] = tree.max_cycle
            key_visibility(obj, keys)
        scene.frame_end = max(scene.frame_end, frames.last_frame(len(trees), start, self.step))
        if self.hide_source:
            source.hide_set(True)
            source.hide_render = True
        scene.frame_set(scene.frame_current)
        self.report({"INFO"}, f"{len(trees)} growth stages in {collection.name}")
        return {"FINISHED"}


CLASSES = (
    ARBRUME_OT_add_plant,
    ARBRUME_OT_regenerate,
    ARBRUME_OT_randomize_seed,
    ARBRUME_OT_duplicate_new_seed,
    ARBRUME_OT_scatter,
    ARBRUME_OT_make_lods,
    ARBRUME_OT_reset_param,
    ARBRUME_OT_curve_mode,
    ARBRUME_OT_export_species,
    ARBRUME_OT_import_species,
    ARBRUME_OT_add_obstacle,
    ARBRUME_OT_remove_obstacle,
    ARBRUME_OT_growth_frames,
)
