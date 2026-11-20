"""Properties stored on plant objects, and the generated species parameter properties.

``Object.arbrume`` (:class:`ARBRUME_PG_plant`) holds what a plant is made from: the species
(a preset name or the text of a species file), seed, age, scale, quality, whether the
plant carries procedural textures, how its branches are joined, and the overrides as JSON
text. Those are saved with the
file, so a plant reopens editable.

``Object.arbrume.params`` is a property group built at registration from the species
schema (:func:`build_params_class`): one property per species parameter, with ``get`` and
``set`` functions that read the effective species and write the overrides. It stores
nothing, so the overrides text stays the single source of truth.
"""

from typing import Any

import bpy
from bpy.props import (
    BoolProperty,
    EnumProperty,
    FloatProperty,
    FloatVectorProperty,
    IntProperty,
    IntVectorProperty,
    PointerProperty,
    StringProperty,
)

from . import plant
from .logic import schema
from .logic.species import readable_name
from .logic.values import (
    MISSING,
    dump_overrides,
    entry_at,
    entry_display_value,
    get_path,
    load_overrides,
    with_entry,
    with_override,
)
from .state import STATE, suspended

MAX_SEED = 2**31 - 1
_ENUM_CACHE: dict[str, list] = {}


def preset_items(_self, _context) -> list:
    """Enum items of the species presets (the list object is kept alive for Blender)."""
    if not _ENUM_CACHE.get("presets"):
        _ENUM_CACHE["presets"] = [
            (name, readable_name(name), f"Species preset {name}", index)
            for index, name in enumerate(STATE.presets)
        ] or [("NONE", "No presets", "The arbrume module did not load", 0)]
    return _ENUM_CACHE["presets"]


def clear_enum_cache() -> None:
    """Forgets the enum item lists, before the extension is unregistered."""
    _ENUM_CACHE.clear()


def _changed(self, _context) -> None:
    plant.regenerate_if_live(self.id_data)


def _get_preset(self) -> int:
    try:
        return STATE.presets.index(self.preset_name)
    except ValueError:
        return 0


def _set_preset(self, value: int) -> None:
    if 0 <= value < len(STATE.presets):
        with suspended():
            self.preset_name = STATE.presets[value]
            self.custom_toml = ""
            self.custom_name = ""
        plant.regenerate_if_live(self.id_data)


class ARBRUME_PG_plant(bpy.types.PropertyGroup):
    """What an Arbrume plant is generated from."""

    is_plant: BoolProperty(name="Arbrume plant", default=False, options={"HIDDEN"})
    preset: EnumProperty(
        name="Species",
        description="Species preset",
        items=preset_items,
        get=_get_preset,
        set=_set_preset,
    )
    preset_name: StringProperty(name="Preset", default="broadleaf")
    custom_toml: StringProperty(
        name="Species file", description="Text of a custom species file", update=_changed
    )
    custom_name: StringProperty(name="Custom species name")
    seed: IntProperty(
        name="Seed", description="Seed of the plant", min=0, max=MAX_SEED, update=_changed
    )
    age: IntProperty(
        name="Age",
        description="Growth cycles; drag to watch the plant grow",
        default=20,
        min=0,
        max=1000,
        soft_max=100,
        update=_changed,
    )
    scale: FloatProperty(
        name="Scale", default=1.0, min=0.01, soft_max=3.0, max=100.0, update=_changed
    )
    quality: FloatProperty(
        name="Quality",
        description="Level of detail, 1 is full detail",
        default=1.0,
        min=0.01,
        soft_max=1.0,
        max=4.0,
        update=_changed,
    )
    textures: BoolProperty(
        name="Textures",
        description="Procedural bark and leaf textures (image, normal map, alpha clip)",
        default=True,
        update=_changed,
    )
    junctions: EnumProperty(
        name="Junctions",
        description="How a branch is joined to its parent",
        items=[
            ("tier-a", "Collar", "Branches start inside their parent with a flared collar"),
            ("tier-b", "Watertight", "One closed bark surface, slower to build"),
        ],
        default="tier-a",
        update=_changed,
    )
    overrides: StringProperty(
        name="Overrides",
        description="Species parameters changed from the species, as JSON",
        default="{}",
        update=_changed,
    )
    live_update: BoolProperty(
        name="Live update", description="Regenerate on every change", default=True
    )
    curve_order: IntProperty(
        name="Order",
        description="Branching order edited by the curve fields (0 is the trunk)",
        min=0,
        max=16,
    )


def settings_of(params: bpy.types.PropertyGroup) -> bpy.types.PropertyGroup:
    """Plant settings that own a parameter property group."""
    return params.id_data.arbrume


def effective_species(settings) -> dict:
    """Species of a plant with its overrides applied."""
    return STATE.cache.effective_text(plant.source_of(settings), settings.overrides)


def current_value(settings, spec: schema.ParamSpec) -> Any:
    """Effective value of a parameter, or its default when the species lacks it."""
    value = get_path(effective_species(settings), spec.path)
    return spec.default if value is MISSING or value is None else value


def write_value(settings, spec: schema.ParamSpec, value: Any) -> None:
    """Sets the override of one parameter; drops it when equal to the species value."""
    base = STATE.cache.base(plant.source_of(settings))
    overrides = with_override(load_overrides(settings.overrides), base, spec.path, value)
    text = dump_overrides(overrides)
    if text != settings.overrides:
        settings.overrides = text


def curve_order_of(settings, spec: schema.ParamSpec) -> int:
    """Branching order a curve field edits: the selected order, or 0 without orders."""
    return int(settings.curve_order) if spec.by_order else 0


def write_curve_entry(
    settings, spec: schema.ParamSpec, entry: Any, order: int | None = None
) -> None:
    """Replaces the curve of one order (the selected one by default) of a curve parameter."""
    value = current_value(settings, spec)
    if spec.by_order:
        orders = schema.order_count(effective_species(settings))
        order = curve_order_of(settings, spec) if order is None else order
        value = with_entry(value, order, entry, orders)
    else:
        value = entry
    write_value(settings, spec, value)


def _accessors(spec: schema.ParamSpec):
    """``get`` and ``set`` functions of the Blender property of one parameter."""

    def get(self):
        settings = settings_of(self)
        value = current_value(settings, spec)
        if spec.kind == schema.CURVE:
            return entry_display_value(entry_at(value, curve_order_of(settings, spec)))
        if spec.kind == schema.ENUM:
            return spec.options.index(value) if value in spec.options else 0
        if spec.kind in (schema.COLOR, schema.VECTOR):
            cast = int if spec.integer else float
            items = list(value) if isinstance(value, list) else list(spec.default)
            return tuple(cast(v) for v in (items + [0] * spec.size)[: spec.size])
        if spec.kind == schema.INT:
            return int(value)
        if spec.kind == schema.BOOL:
            return bool(value)
        return float(value)

    def set_(self, value):
        settings = settings_of(self)
        if spec.kind == schema.CURVE:
            write_curve_entry(settings, spec, float(value))
        elif spec.kind == schema.ENUM:
            write_value(settings, spec, spec.options[value])
        elif spec.kind in (schema.COLOR, schema.VECTOR):
            write_value(settings, spec, [int(v) if spec.integer else float(v) for v in value])
        elif spec.kind == schema.INT:
            write_value(settings, spec, int(value))
        elif spec.kind == schema.BOOL:
            write_value(settings, spec, bool(value))
        else:
            write_value(settings, spec, float(value))

    return get, set_


def _range(spec: schema.ParamSpec) -> dict:
    low = spec.soft_min if spec.soft_min is not None else 0.0
    high = spec.soft_max if spec.soft_max is not None else max(1.0, low + 1.0)
    if spec.integer or spec.kind == schema.INT:
        return {"soft_min": int(low), "soft_max": int(high)}
    return {"soft_min": low, "soft_max": high, "step": max(0.1, high - low), "precision": 3}


def make_property(spec: schema.ParamSpec):
    """The Blender property that edits one parameter."""
    get, set_ = _accessors(spec)
    common = {"name": spec.label, "description": spec.description or spec.path}
    common.update(get=get, set=set_)
    if spec.kind == schema.BOOL:
        return BoolProperty(**common)
    if spec.kind == schema.ENUM:
        items = [(o, o.replace("_", " ").capitalize(), o, i) for i, o in enumerate(spec.options)]
        _ENUM_CACHE[spec.path] = items
        return EnumProperty(items=items, **common)
    if spec.kind == schema.COLOR:
        return FloatVectorProperty(subtype="COLOR", size=spec.size, min=0.0, max=1.0, **common)
    if spec.kind == schema.VECTOR:
        kind = IntVectorProperty if spec.integer else FloatVectorProperty
        return kind(size=spec.size, **_range(spec), **common)
    if spec.kind == schema.INT:
        return IntProperty(**_range(spec), **common)
    return FloatProperty(**_range(spec), **common)


def build_params_class(sections: list[schema.Section]) -> type:
    """A property group with one property per parameter of ``sections``."""
    annotations = {
        spec.ident: make_property(spec) for section in sections for spec in section.params()
    }
    return type(
        "ARBRUME_PG_params",
        (bpy.types.PropertyGroup,),
        {"__annotations__": annotations, "__doc__": "Species parameters of a plant"},
    )


def attach_params(params_class: type) -> None:
    """Adds the parameter group to the plant settings once both are registered."""
    ARBRUME_PG_plant.params = PointerProperty(type=params_class)


def detach_params() -> None:
    """Removes the parameter group from the plant settings."""
    if hasattr(ARBRUME_PG_plant, "params"):
        del ARBRUME_PG_plant.params
