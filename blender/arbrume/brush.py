"""The Arbrume Brush: strokes painted in the 3D view that steer self-organizing growth.

The tool (toolbar of the 3D view in Object Mode, or *Brush Tool* in the sidebar) runs
:class:`ARBRUME_OT_brush_stroke` on each left-button press: the stroke follows the mouse
until release, projected onto a plane parallel to the screen through the point of the
plant (or of any surface) under the first point, or through the plant origin
(:mod:`.logic.brush`), or onto the surfaces under the mouse. While the button is held, the
plant grows again with the stroke so far at the *Preview Quality*, as often as the
generator keeps up; on release the stroke is stored on the plant (``Object.arbrume.guides``,
saved with the file) and the plant grows at its own quality. Each stroke is one undo step.

Modes: *Crown* paints markers of free space the crown grows into, *Limb* draws a line that
one axis follows (start it on the trunk or a branch, or at the ground for the trunk), and
*Erase* removes growth and keeps it out. *Freeze* keeps the plant grown so far and lets
later strokes only add growth, over *Growth After Freeze* more cycles.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from typing import ClassVar

import bpy
import gpu
import numpy as np
from bpy.props import (
    BoolProperty,
    EnumProperty,
    FloatProperty,
    FloatVectorProperty,
    IntProperty,
    StringProperty,
)
from bpy_extras import view3d_utils
from gpu_extras.batch import batch_for_shader

from . import plant
from .logic import brush as logic
from .logic.species import Source
from .state import STATE, status_of, suspended

TOOL_ID = "arbrume.brush_tool"
MODES = [
    ("CROWN", "Crown", "Paint where the crown grows", "OUTLINER_OB_POINTCLOUD", 0),
    ("LIMB", "Limb", "Draw a line one branch or the trunk follows", "CURVE_BEZCURVE", 1),
    ("ERASE", "Erase", "Remove growth and keep it out", "X", 2),
]
COLORS = {
    "crown": (0.35, 0.9, 0.35, 0.9),
    "limb": (1.0, 0.6, 0.2, 0.95),
    "erase": (1.0, 0.25, 0.25, 0.9),
}
UNDO = {"REGISTER", "UNDO"}


@dataclass
class StrokeTiming:
    """Generation times of the last stroke of a plant, in milliseconds."""

    previews: list[float] = field(default_factory=list)
    final: float = 0.0
    upload: float = 0.0


TIMINGS: dict[str, StrokeTiming] = {}
"""Timings of the last stroke of each plant, by object name."""
DRAWING: dict[str, list] = {"points": [], "mode": []}
"""The stroke being drawn, in world space, for the overlay."""


class ARBRUME_PG_brush(bpy.types.PropertyGroup):
    """Settings of the Arbrume Brush, shared by the plants of a scene."""

    mode: EnumProperty(name="Mode", items=MODES, default="CROWN")
    radius: FloatProperty(
        name="Radius",
        description="Radius of the brush: of the painted or erased volume, and the distance "
        "within which a limb finds its branch",
        default=0.6,
        min=0.01,
        soft_max=5.0,
        subtype="DISTANCE",
    )
    strength: FloatProperty(
        name="Strength",
        description="Light given to the buds a stroke serves, as a multiple of full "
        "exposure (crown) or of the light of the whole plant (limb)",
        default=1.5,
        min=0.0,
        soft_max=5.0,
    )
    reach: FloatProperty(
        name="Reach",
        description="Distance from which buds are drawn to a crown stroke, 0 for the "
        "perception distance of the species",
        default=4.0,
        min=0.0,
        soft_max=15.0,
        subtype="DISTANCE",
    )
    density: FloatProperty(
        name="Density",
        description="Markers of a crown stroke (and leafy shoots along a limb), as a "
        "multiple of the marker density of the species",
        default=1.0,
        min=0.0,
        soft_max=4.0,
    )
    follow: FloatProperty(
        name="Follow",
        description="How closely a limb keeps to its line: 1 exactly, 0 not at all",
        default=0.9,
        min=0.0,
        max=1.0,
    )
    projection: EnumProperty(
        name="Projection",
        items=[
            (
                "PLANE",
                "Plane",
                "A plane facing the view through the point under the stroke "
                "start, or through the plant",
            ),
            ("SURFACE", "Surface", "The surfaces under the mouse; the plane elsewhere"),
        ],
        default="PLANE",
    )
    erase_through: BoolProperty(
        name="Erase Through",
        description="Erase everything under the brush along the view, not only near the "
        "drawing plane",
        default=True,
    )
    preview_quality: FloatProperty(
        name="Preview Quality",
        description="Mesh quality while the button is held; the plant's own on release",
        default=0.25,
        min=0.01,
        max=1.0,
    )
    spacing: IntProperty(
        name="Spacing",
        description="Pixels the mouse moves before a stroke takes a new point",
        default=6,
        min=1,
        soft_max=40,
    )
    grow_after_freeze: IntProperty(
        name="Growth After Freeze",
        description="Cycles added to the age by Freeze, in which later strokes grow",
        default=6,
        min=1,
        soft_max=30,
    )
    show_strokes: BoolProperty(
        name="Show Strokes",
        description="Draw the strokes of the active plant when the brush tool is not active",
        default=False,
    )


def settings_of(context) -> ARBRUME_PG_brush:
    """Brush settings of the scene."""
    return context.scene.arbrume_brush


def stroke_options(settings) -> dict[str, float]:
    """Numbers a new stroke takes from the brush settings."""
    return {
        "density": settings.density,
        "strength": settings.strength,
        "follow": settings.follow,
        "reach": settings.reach,
    }


def guides_of(obj) -> dict:
    """Guides stored on a plant."""
    return logic.load(obj.arbrume.guides)


def is_steerable(obj) -> bool:
    """Whether the species of a plant grows in the self-organizing mode."""
    settings = obj.arbrume
    species = STATE.cache.effective_text(plant.source_of(settings), settings.overrides)
    return logic.is_self_organizing(species)


def plant_points(obj, world_points, radius: float, even: bool = True) -> tuple[np.ndarray, float]:
    """World points of a stroke and a world radius, in the frame of a plant; resampled
    every quarter radius when ``even``."""
    matrix = np.array(obj.matrix_world)
    scale = logic.plant_scale(matrix)
    local = logic.world_to_plant(matrix, np.asarray(world_points, dtype=np.float64))
    radius = radius / scale
    return (logic.resample(local, 0.25 * radius) if even else local), radius


def erased_through(obj, world_points, radius: float, view) -> dict:
    """The guides of a plant with an erase stroke that cuts along the view direction
    ``view`` through the whole plant: one straight erase stroke per half radius."""
    depth = float(max(obj.dimensions)) + 2.0 * radius
    guides = guides_of(obj)
    for segment in logic.swept(world_points, view, depth, 0.5 * radius):
        points, local_radius = plant_points(obj, segment, radius, even=False)
        guides = logic.with_stroke(guides, logic.make_stroke("ERASE", points, local_radius, {}))
    return guides


def guides_with(
    obj, mode: str, world_points, settings, radius: float | None = None, view=None
) -> dict:
    """The guides of a plant with one more stroke of ``mode`` through ``world_points``;
    an erase stroke cuts through the plant along ``view`` when the brush erases through."""
    radius = settings.radius if radius is None else radius
    if mode == "ERASE" and view is not None and settings.erase_through:
        return erased_through(obj, world_points, radius, view)
    points, local_radius = plant_points(obj, world_points, radius)
    stroke = logic.make_stroke(mode, points, local_radius, stroke_options(settings))
    if stroke["kind"] == "limb" and len(stroke["points"]) < 2:
        raise ValueError("a limb stroke needs two distinct points")
    return logic.with_stroke(guides_of(obj), stroke)


def write_guides(obj, guides: dict) -> str:
    """Stores ``guides`` on a plant and grows it again; returns the generator error."""
    with suspended():
        obj.arbrume.guides = logic.dump(guides)
    return plant.regenerate(obj)


def add_stroke(
    obj, mode: str, world_points, settings, radius: float | None = None, view=None
) -> str:
    """Adds a stroke drawn through ``world_points`` (seen along ``view``, for an erase
    stroke that cuts through) to a plant and grows it again; returns the generator error.
    The entry point of scripts and tests."""
    return write_guides(obj, guides_with(obj, mode, world_points, settings, radius, view))


def view_ray(context, coord) -> tuple[np.ndarray, np.ndarray]:
    """Origin and direction of the view ray through the region pixel ``coord``."""
    region, rv3d = context.region, context.region_data
    origin = view3d_utils.region_2d_to_origin_3d(region, rv3d, coord)
    direction = view3d_utils.region_2d_to_vector_3d(region, rv3d, coord)
    return np.array(origin), np.array(direction)


def surface_hit(context, origin, direction) -> np.ndarray | None:
    """First point of a visible object of the scene along a ray, or ``None``."""
    depsgraph = context.evaluated_depsgraph_get()
    hit, location, *_ = context.scene.ray_cast(depsgraph, tuple(origin), tuple(direction))
    return np.array(location) if hit else None


class ARBRUME_OT_brush_stroke(bpy.types.Operator):
    """Paint a stroke that steers the growth of the active plant"""

    bl_idname = "arbrume.brush_stroke"
    bl_label = "Arbrume Brush Stroke"
    bl_options: ClassVar[set[str]] = UNDO

    mode: EnumProperty(name="Mode", items=MODES, default="CROWN")
    points: StringProperty(
        name="Points",
        description="World points of the stroke, as JSON",
        options={"HIDDEN", "SKIP_SAVE"},
    )
    view: FloatVectorProperty(
        name="View",
        description="Direction of the view the stroke was drawn in, zero for none",
        size=3,
        options={"HIDDEN", "SKIP_SAVE"},
    )

    @classmethod
    def poll(cls, context):
        obj = getattr(context, "active_object", None)
        return plant.is_plant(obj) and plant.is_editable(obj)

    def view_direction(self) -> np.ndarray | None:
        """The view direction of the stroke, ``None`` when it was not given."""
        view = np.array(self.view, dtype=np.float64)
        return view if np.linalg.norm(view) > 0.0 else None

    def execute(self, context):
        obj = context.active_object
        world = json.loads(self.points or "[]")
        if not world:
            return {"CANCELLED"}
        settings = settings_of(context)
        try:
            guides = guides_with(obj, self.mode, world, settings, view=self.view_direction())
        except ValueError as error:
            self.report({"WARNING"}, str(error))
            return {"CANCELLED"}
        start = time.perf_counter()
        error = write_guides(obj, guides)
        timing = TIMINGS.setdefault(obj.name, StrokeTiming())
        timing.final = (time.perf_counter() - start) * 1e3
        timing.upload = status_of(obj.name).upload_ms
        if error:
            self.report({"ERROR"}, error)
        return {"FINISHED"}

    def invoke(self, context, event):
        obj = context.active_object
        if not is_steerable(obj):
            self.report({"WARNING"}, "Strokes steer self-organizing species only")
            return {"CANCELLED"}
        if context.region_data is None:
            return {"CANCELLED"}
        settings = settings_of(context)
        self.mode = settings.mode
        coord = (event.mouse_region_x, event.mouse_region_y)
        origin, direction = view_ray(context, coord)
        anchor = surface_hit(context, origin, direction)
        if anchor is None:
            anchor = np.array(obj.matrix_world.translation)
        self._plane = logic.drawing_plane(direction, anchor)
        self.view = tuple(self._plane[1])
        self._world: list = []
        self._last = coord
        self._dirty = False
        self._settings = settings
        TIMINGS[obj.name] = StrokeTiming()
        self._add_point(context, coord)
        self._timer = context.window_manager.event_timer_add(1 / 60, window=context.window)
        context.window_manager.modal_handler_add(self)
        return {"RUNNING_MODAL"}

    def _add_point(self, context, coord) -> None:
        origin, direction = view_ray(context, coord)
        point = None
        if self._settings.projection == "SURFACE":
            point = surface_hit(context, origin, direction)
        if point is None:
            point = logic.ray_plane(origin, direction, *self._plane)
        if point is not None:
            self._world.append([float(v) for v in point])
            self._dirty = True
            DRAWING["points"], DRAWING["mode"] = self._world, [self.mode]

    def _preview(self, context) -> None:
        obj = context.active_object
        self._dirty = False
        try:
            view = self.view_direction()
            guides = guides_with(obj, self.mode, self._world, self._settings, view=view)
        except ValueError:
            return
        start = time.perf_counter()
        quality = min(self._settings.preview_quality, obj.arbrume.quality)
        plant.regenerate(obj, {"guides": guides, "quality": quality})
        TIMINGS[obj.name].previews.append((time.perf_counter() - start) * 1e3)
        context.area.tag_redraw()

    def _finish(self, context) -> None:
        context.window_manager.event_timer_remove(self._timer)
        DRAWING["points"], DRAWING["mode"] = [], []
        context.area.tag_redraw()

    def modal(self, context, event):
        if event.type == "MOUSEMOVE":
            coord = (event.mouse_region_x, event.mouse_region_y)
            moved = np.hypot(coord[0] - self._last[0], coord[1] - self._last[1])
            if moved >= self._settings.spacing:
                self._last = coord
                self._add_point(context, coord)
        elif event.type == "TIMER" and self._dirty:
            self._preview(context)
        elif event.type == "LEFTMOUSE" and event.value == "RELEASE":
            self._finish(context)
            self.points = json.dumps(self._world)
            return self.execute(context)
        elif event.type in {"ESC", "RIGHTMOUSE"}:
            self._finish(context)
            plant.regenerate(context.active_object)
            return {"CANCELLED"}
        return {"RUNNING_MODAL"}


class ARBRUME_OT_clear_strokes(bpy.types.Operator):
    """Remove every stroke and freeze of the active plant"""

    bl_idname = "arbrume.clear_strokes"
    bl_label = "Clear Strokes"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        obj = getattr(context, "active_object", None)
        return plant.is_plant(obj) and bool(obj.arbrume.guides)

    def execute(self, context):
        write_guides(context.active_object, logic.load(""))
        return {"FINISHED"}


class ARBRUME_OT_freeze_growth(bpy.types.Operator):
    """Keep the plant grown so far: later strokes only add growth, over more cycles"""

    bl_idname = "arbrume.freeze_growth"
    bl_label = "Freeze"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        obj = getattr(context, "active_object", None)
        return plant.is_plant(obj) and plant.is_editable(obj)

    def execute(self, context):
        obj = context.active_object
        settings = obj.arbrume
        guides = logic.with_freeze(guides_of(obj), int(settings.age))
        with suspended():
            settings.age = int(settings.age) + settings_of(context).grow_after_freeze
        write_guides(obj, guides)
        return {"FINISHED"}


class ARBRUME_OT_unfreeze_growth(bpy.types.Operator):
    """Remove the last freeze and the strokes drawn after it, back to the frozen age"""

    bl_idname = "arbrume.unfreeze_growth"
    bl_label = "Unfreeze"
    bl_options: ClassVar[set[str]] = UNDO

    @classmethod
    def poll(cls, context):
        obj = getattr(context, "active_object", None)
        return plant.is_plant(obj) and bool(guides_of(obj).get("freezes"))

    def execute(self, context):
        obj = context.active_object
        guides, age = logic.without_last_freeze(guides_of(obj))
        if age is not None:
            with suspended():
                obj.arbrume.age = age
        write_guides(obj, guides)
        return {"FINISHED"}


def nearest_preset(obj) -> str | None:
    """The self-organizing preset closest in habit to the species of a plant."""
    settings = obj.arbrume
    species = STATE.cache.effective_text(plant.source_of(settings), settings.overrides)
    presets = {name: STATE.cache.base(Source(preset=name)) for name in STATE.presets}
    return logic.nearest_self_organizing(species, presets)


class ARBRUME_OT_switch_self_organizing(bpy.types.Operator):
    """Use the self-organizing preset closest in habit, which strokes can steer"""

    bl_idname = "arbrume.switch_self_organizing"
    bl_label = "Switch to Self-Organizing"
    bl_options: ClassVar[set[str]] = UNDO

    preset: StringProperty(name="Preset", options={"SKIP_SAVE"})

    @classmethod
    def poll(cls, context):
        return plant.is_plant(getattr(context, "active_object", None))

    def execute(self, context):
        obj = context.active_object
        name = self.preset or nearest_preset(obj)
        if not name:
            self.report({"WARNING"}, "No self-organizing preset is available")
            return {"CANCELLED"}
        with suspended():
            obj.arbrume.preset_name = name
            obj.arbrume.custom_toml = ""
            obj.arbrume.custom_name = ""
            obj.arbrume.overrides = "{}"
        plant.regenerate(obj)
        return {"FINISHED"}


def tool_active(context) -> bool:
    """Whether the brush tool is the active tool of the 3D view."""
    workspace = getattr(context, "workspace", None)
    if workspace is None or context.mode != "OBJECT":
        return False
    tool = workspace.tools.from_space_view3d_mode("OBJECT", create=False)
    return tool is not None and tool.idname == TOOL_ID


def draw_settings(layout, settings, compact: bool) -> None:
    """The brush settings, in the tool header (``compact``) or the sidebar."""
    layout.prop(settings, "mode", expand=True)
    layout.prop(settings, "radius")
    if settings.mode != "ERASE":
        layout.prop(settings, "strength")
    if settings.mode == "CROWN":
        layout.prop(settings, "reach")
    if settings.mode != "ERASE":
        layout.prop(settings, "density")
    if settings.mode == "LIMB":
        layout.prop(settings, "follow")
    if settings.mode == "ERASE":
        layout.prop(settings, "erase_through")
    layout.prop(settings, "projection", text="" if compact else "Projection")
    if not compact:
        layout.prop(settings, "preview_quality")
        layout.prop(settings, "spacing")
        layout.prop(settings, "grow_after_freeze")
        layout.prop(settings, "show_strokes")


class ARBRUME_TL_brush(bpy.types.WorkSpaceTool):
    """Paint strokes that steer the growth of the active Arbrume plant"""

    bl_space_type = "VIEW_3D"
    bl_context_mode = "OBJECT"
    bl_idname = TOOL_ID
    bl_label = "Arbrume Brush"
    bl_description = "Paint crowns, draw limbs and erase growth of the active Arbrume plant"
    bl_icon = "ops.gpencil.draw"
    bl_keymap = (("arbrume.brush_stroke", {"type": "LEFTMOUSE", "value": "PRESS"}, None),)

    @staticmethod
    def draw_settings(context, layout, _tool):
        draw_settings(layout, settings_of(context), compact=True)


def _report_lines(obj) -> list[str]:
    """What the strokes of a plant did, as warnings for the strokes that did nothing."""
    report = status_of(obj.name).guides or {}
    lines = []
    for index, stroke in enumerate(report.get("strokes", [])):
        kinds = [s.get("kind") for s in guides_of(obj).get("strokes", [])]
        kind = kinds[index] if index < len(kinds) else ""
        if kind == "limb" and not stroke.get("claimed"):
            lines.append(f"Limb {index + 1}: no branch within its radius of its start")
        elif kind == "limb" and stroke["followed"] < 0.9 * stroke["length"]:
            share = stroke["followed"] / max(stroke["length"], 1e-6)
            lines.append(f"Limb {index + 1}: grown along {share:.0%}; raise the age")
    return lines


def _draw_strokes_box(layout, obj) -> None:
    guides = guides_of(obj)
    counts = logic.counts(guides)
    box = layout.box()
    box.label(
        text=f"{counts['crown']} crown, {counts['limb']} limb, {counts['erase']} erase strokes",
        icon="GREASEPENCIL",
    )
    freezes = guides.get("freezes") or []
    if freezes:
        box.label(text=f"Frozen at age {freezes[-1]}", icon="FREEZE")
    row = box.row(align=True)
    row.operator("arbrume.freeze_growth", icon="FREEZE")
    row.operator("arbrume.unfreeze_growth", text="", icon="LOOP_BACK")
    row.operator("arbrume.clear_strokes", text="", icon="TRASH")
    timing = TIMINGS.get(obj.name)
    if timing is not None and timing.final:
        previews = timing.previews
        mean = sum(previews) / len(previews) if previews else 0.0
        box.label(
            text=f"Last stroke: {len(previews)} previews, {mean:.0f} ms each; "
            f"{timing.final:.0f} ms on release",
            icon="TIME",
        )
    for line in _report_lines(obj):
        box.label(text=line, icon="ERROR")


class VIEW3D_PT_arbrume_brush(bpy.types.Panel):
    """Brush strokes that steer the growth of the plant"""

    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Arbrume"
    bl_label = "Brush"
    bl_parent_id = "VIEW3D_PT_arbrume"
    bl_options: ClassVar[set[str]] = {"DEFAULT_CLOSED"}

    @classmethod
    def poll(cls, context):
        return plant.is_plant(getattr(context, "active_object", None))

    def draw(self, context):
        obj = context.active_object
        layout = self.layout
        layout.enabled = plant.is_editable(obj)
        if not is_steerable(obj):
            layout.label(text="Strokes steer self-organizing species", icon="INFO")
            name = nearest_preset(obj)
            if name:
                op = layout.operator("arbrume.switch_self_organizing", text=f"Switch to {name}")
                op.preset = name
            return
        active = tool_active(context)
        layout.operator(
            "wm.tool_set_by_id",
            text="Brush Tool",
            icon="GREASEPENCIL",
            depress=active,
        ).name = TOOL_ID
        col = layout.column()
        col.use_property_split = True
        col.use_property_decorate = False
        draw_settings(col, settings_of(context), compact=False)
        _draw_strokes_box(layout, obj)


def _polyline(shader, points, color, width: float, viewport) -> None:
    if len(points) < 2:
        points = [*points, *points]
    batch = batch_for_shader(shader, "LINE_STRIP", {"pos": [tuple(p) for p in points]})
    shader.uniform_float("viewportSize", viewport)
    shader.uniform_float("lineWidth", width)
    shader.uniform_float("color", color)
    gpu.state.blend_set("ALPHA")
    batch.draw(shader)


def draw_overlay() -> None:
    """Draws the strokes of the active plant and the stroke being drawn."""
    context = bpy.context
    obj = getattr(context, "active_object", None)
    if not plant.is_plant(obj):
        return
    scene = getattr(context, "scene", None)
    showing = scene is not None and settings_of(context).show_strokes
    if not (tool_active(context) or showing or DRAWING["points"]):
        return
    shader = gpu.shader.from_builtin("POLYLINE_UNIFORM_COLOR")
    region = context.region
    viewport = (region.width, region.height)
    matrix = np.array(obj.matrix_world)
    gpu.state.depth_test_set("NONE")
    for stroke in guides_of(obj).get("strokes", []):
        world = logic.plant_to_world(matrix, np.asarray(stroke["points"], dtype=np.float64))
        _polyline(shader, world, COLORS.get(stroke["kind"], (1, 1, 1, 1)), 2.5, viewport)
    if DRAWING["points"]:
        kind = logic.KINDS[DRAWING["mode"][0]]
        _polyline(shader, DRAWING["points"], COLORS[kind], 4.0, viewport)
    gpu.state.blend_set("NONE")


CLASSES = (
    ARBRUME_PG_brush,
    ARBRUME_OT_brush_stroke,
    ARBRUME_OT_clear_strokes,
    ARBRUME_OT_freeze_growth,
    ARBRUME_OT_unfreeze_growth,
    ARBRUME_OT_switch_self_organizing,
)
_HANDLES: list = []


def register() -> None:
    """Registers the brush properties, operators, tool and overlay (panels apart)."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.arbrume_brush = bpy.props.PointerProperty(type=ARBRUME_PG_brush)
    bpy.utils.register_tool(ARBRUME_TL_brush, separator=True)
    if not bpy.app.background:
        space = bpy.types.SpaceView3D
        _HANDLES.append(space.draw_handler_add(draw_overlay, (), "WINDOW", "POST_VIEW"))


def unregister() -> None:
    """Removes everything :func:`register` added."""
    for handle in _HANDLES:
        bpy.types.SpaceView3D.draw_handler_remove(handle, "WINDOW")
    _HANDLES.clear()
    bpy.utils.unregister_tool(ARBRUME_TL_brush)
    if hasattr(bpy.types.Scene, "arbrume_brush"):
        del bpy.types.Scene.arbrume_brush
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
    TIMINGS.clear()
