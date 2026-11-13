"""Sidebar panels (3D view, "Arbrume" tab) and the Add > Mesh menu entry.

The main panel edits what a plant is made from. One collapsible sub-panel per section of
the species (architecture, growth, geometry, foliage, bark, and any section the species
data gains) is built at registration from the schema, with a collapsible block per group.
"""

from __future__ import annotations

from typing import ClassVar

import bpy

from . import curves_ui, plant, props
from .logic import schema
from .logic.values import entry_at, entry_points, f32_text, load_overrides
from .ops import active_plant
from .state import STATE, status_of

CATEGORY = "Arbrume"
MAIN_PANEL = "VIEW3D_PT_arbrume"
OPEN_ALL_GROUPS = False
"""Open every parameter group by default (the first group of a section always is)."""


def _overridden(overrides: dict, path: str) -> bool:
    return any(key == path or key.startswith(path + ".") for key in overrides)


def _status(layout, obj) -> None:
    status = status_of(obj.name)
    if status.error:
        box = layout.box()
        box.alert = True
        for line in status.error.splitlines()[:6]:
            box.label(text=line, icon="ERROR" if line is status.error.splitlines()[0] else "BLANK1")
    elif status.triangles:
        layout.label(
            text=f"{status.triangles:,} triangles, {status.generate_ms:.0f} ms + "
            f"{status.upload_ms:.0f} ms upload",
            icon="INFO",
        )


def _age_hint(layout, settings) -> None:
    base = STATE.cache.base(plant.source_of(settings))
    ages = base.get("variation", {}).get("age")
    if isinstance(ages, list) and len(ages) == 2:
        layout.label(text=f"Suggested ages: {ages[0]} to {ages[1]}")


class VIEW3D_PT_arbrume(bpy.types.Panel):
    """Arbrume plant settings"""

    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = CATEGORY
    bl_label = "Arbrume"

    def draw(self, context):
        layout = self.layout
        if STATE.schema_error:
            layout.label(text=STATE.schema_error, icon="ERROR")
        obj = active_plant(context)
        if obj is None:
            layout.operator("arbrume.add_plant", icon="ADD")
            layout.operator("arbrume.import_species", icon="FILE_FOLDER")
            layout.label(text="Select a plant to edit it")
            return
        settings = obj.arbrume
        layout.enabled = plant.is_editable(obj)
        col = layout.column()
        col.use_property_split = True
        col.use_property_decorate = False
        if settings.custom_toml:
            col.label(text=f"Custom species: {settings.custom_name}", icon="FILE_TEXT")
            col.prop(settings, "preset", text="Use Preset")
        else:
            col.prop(settings, "preset")
        row = col.row(align=True)
        row.prop(settings, "seed")
        row.operator("arbrume.randomize_seed", text="", icon="FILE_REFRESH")
        col.prop(settings, "age", slider=True)
        _age_hint(col, settings)
        col.prop(settings, "scale")
        col.prop(settings, "quality", slider=True)
        row = layout.row(align=True)
        row.prop(settings, "live_update", toggle=True, icon="PLAY")
        row.operator("arbrume.regenerate", icon="FILE_REFRESH")
        _status(layout, obj)
        if load_overrides(settings.overrides):
            op = layout.operator(
                "arbrume.reset_param", text="Reset All Parameters", icon="LOOP_BACK"
            )
            op.path = ""


class VIEW3D_PT_arbrume_tools(bpy.types.Panel):
    """Variations, batches, levels of detail and species files"""

    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = CATEGORY
    bl_label = "Batch and Files"
    bl_parent_id = MAIN_PANEL
    bl_options: ClassVar[set[str]] = {"DEFAULT_CLOSED"}

    @classmethod
    def poll(cls, context):
        return active_plant(context) is not None

    def draw(self, _context):
        col = self.layout.column(align=True)
        col.operator("arbrume.duplicate_new_seed", icon="DUPLICATE")
        col.operator("arbrume.scatter", icon="GRID")
        col.operator("arbrume.make_lods", icon="MOD_DECIM")
        col = self.layout.column(align=True)
        col.operator("arbrume.export_species", icon="EXPORT")
        col.operator("arbrume.import_species", icon="IMPORT")


def _draw_curve(layout, obj, spec: schema.ParamSpec, overridden: bool) -> None:
    settings = obj.arbrume
    order = props.curve_order_of(settings, spec)
    entry = entry_at(props.current_value(settings, spec), order)
    row = layout.row(align=True)
    if entry_points(entry) is None:
        row.prop(settings.params, spec.ident, text=spec.label)
        op = row.operator("arbrume.curve_mode", text="", icon="FCURVE")
        op.path, op.to_curve = spec.path, True
        _reset_button(row, spec, overridden)
        return
    row.label(text=spec.label + (f" (order {order})" if spec.by_order else ""))
    op = row.operator("arbrume.curve_mode", text="", icon="IPO_CONSTANT")
    op.path, op.to_curve = spec.path, False
    _reset_button(row, spec, overridden)
    node = curves_ui.request(obj, spec.path, order)
    span = curves_ui.binding_range(curves_ui.binding_name(obj.name, spec.path, order))
    if node is None or span is None:
        layout.label(text="Opening curve editor...", icon="TIME")
        return
    layout.template_curve_mapping(node, "mapping")
    layout.label(text=f"Bottom {f32_text(span[0])}, top {f32_text(span[1])}")


def _reset_button(row, spec: schema.ParamSpec, overridden: bool) -> None:
    sub = row.row(align=True)
    sub.enabled = overridden
    op = sub.operator("arbrume.reset_param", text="", icon="LOOP_BACK")
    op.path = spec.path


def draw_param(layout, obj, spec: schema.ParamSpec, overrides: dict) -> None:
    """One parameter row: its field (or curve widget) and its reset button."""
    overridden = _overridden(overrides, spec.path)
    if spec.kind == schema.CURVE:
        _draw_curve(layout, obj, spec, overridden)
        return
    row = layout.row(align=True)
    row.prop(obj.arbrume.params, spec.ident, text=spec.label)
    _reset_button(row, spec, overridden)


def make_section_panel(section: schema.Section) -> type:
    """A sub-panel class for one section of the species."""
    has_orders = any(spec.by_order for spec in section.params())

    def draw(self, context):
        obj = active_plant(context)
        if obj is None or not hasattr(obj.arbrume, "params"):
            return
        layout = self.layout
        layout.enabled = plant.is_editable(obj)
        layout.use_property_split = True
        layout.use_property_decorate = False
        overrides = load_overrides(obj.arbrume.overrides)
        if has_orders:
            layout.prop(obj.arbrume, "curve_order", text="Curves of order")
        for index, (group, specs) in enumerate(section.groups.items()):
            body = layout.column()
            if group:
                closed = index > 0 and not OPEN_ALL_GROUPS
                header, body = layout.panel(
                    f"arbrume_{section.name}_{group}", default_closed=closed
                )
                header.label(text=group)
                if body is None:
                    continue
            for spec in specs:
                draw_param(body, obj, spec, overrides)

    return type(
        f"VIEW3D_PT_arbrume_{section.name}",
        (bpy.types.Panel,),
        {
            "__doc__": f"{section.label} parameters of the species",
            "bl_space_type": "VIEW_3D",
            "bl_region_type": "UI",
            "bl_category": CATEGORY,
            "bl_label": section.label,
            "bl_parent_id": MAIN_PANEL,
            "bl_options": {"DEFAULT_CLOSED"},
            "poll": classmethod(lambda cls, context: active_plant(context) is not None),
            "draw": draw,
        },
    )


def menu_add(self, _context) -> None:
    """Entry of Add > Mesh."""
    self.layout.operator("arbrume.add_plant", icon="OUTLINER_OB_POINTCLOUD")


CLASSES = (VIEW3D_PT_arbrume, VIEW3D_PT_arbrume_tools)
