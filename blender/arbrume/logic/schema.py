"""The editable parameters of a species, found from the data rather than listed by hand.

:func:`build_schema` walks the full species of every preset (``tree.metadata["species"]``,
which includes the defaults) and asks the generator itself what each field accepts: a field
that takes a list of ``[x, y]`` points is a curve, a curve that takes ``{by_order = [...]}``
has one curve per branching order, and the error for an unknown string lists the variants
of an enumeration. A parameter added to the Rust schema therefore shows up in the user
interface without any change here; :mod:`.uimeta` only adds labels and slider ranges.
"""

from __future__ import annotations

import re
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from typing import Any

from . import uimeta
from .values import MISSING, get_path, is_curve_value, order_entries

FLOAT, INT, BOOL, ENUM, COLOR, VECTOR, CURVE = (
    "float",
    "int",
    "bool",
    "enum",
    "color",
    "vector",
    "curve",
)

Probe = Callable[[str, Any], str | None]
"""``probe(path, value)``: None when the override is accepted, else the error message."""


@dataclass
class ParamSpec:
    """One editable parameter and how to show it."""

    path: str
    kind: str
    label: str
    group: str
    default: Any
    options: tuple[str, ...] = ()
    size: int = 0
    integer: bool = False
    by_order: bool = False
    soft_min: float | None = None
    soft_max: float | None = None
    description: str = ""

    @property
    def ident(self) -> str:
        """Identifier of the Blender property that edits it."""
        return self.path.replace(".", "__")

    @property
    def section(self) -> str:
        """Top-level table holding the parameter."""
        return self.path.split(".", 1)[0]


@dataclass
class Section:
    """A top-level table of the species and its parameters, grouped in display order."""

    name: str
    label: str
    groups: dict[str, list[ParamSpec]] = field(default_factory=dict)

    def params(self) -> list[ParamSpec]:
        """Every parameter of the section, in display order."""
        return [spec for group in self.groups.values() for spec in group]


def _is_number(value: Any) -> bool:
    return isinstance(value, int | float) and not isinstance(value, bool)


def collect_fields(species_list: Iterable[dict]) -> dict[str, list[Any]]:
    """Every leaf parameter path of the species, with the values seen for it.

    The walk stops at values (numbers, strings, lists, curves); a table that is not a
    curve is walked into. Arrays of tables (``architecture.axes``) are left out: they are
    edited in a species file.
    """
    found: dict[str, list[Any]] = {}

    def walk(node: dict, prefix: str) -> None:
        for key, value in node.items():
            path = f"{prefix}{key}"
            if not prefix and key in uimeta.HIDDEN_TOP_LEVEL:
                continue
            if isinstance(value, dict) and not is_curve_value(value):
                walk(value, path + ".")
            elif isinstance(value, list) and value and all(isinstance(v, dict) for v in value):
                continue
            elif prefix:
                found.setdefault(path, []).append(value)

    for species in species_list:
        walk(species, "")
    return found


def enum_options(message: str) -> tuple[str, ...]:
    """Variant names listed in a serde error such as "expected `a` or `b`"."""
    _, _, tail = message.partition("expected")
    tail = tail.split("\n", 1)[0]
    return tuple(re.findall(r"`([^`]+)`", tail))


def _numeric_kind(path: str, values: list[Any], probe: Probe) -> ParamSpec:
    curves = [v for v in values if is_curve_value(v)]
    numbers = [v for v in values if _is_number(v)]
    sample = float(numbers[0]) if numbers else 0.0
    integer = bool(numbers) and not curves and all(isinstance(v, int) for v in numbers)
    is_curve = bool(curves) or (not integer and probe(path, [[0.0, sample], [1.0, sample]]) is None)
    by_order = is_curve and (
        any(isinstance(v, dict) and "by_order" in v for v in curves)
        or probe(path, {"by_order": [sample]}) is None
    )
    default = values[0]
    meta = uimeta.meta_for(path, sample, integer)
    return ParamSpec(
        path=path,
        kind=CURVE if is_curve else (INT if integer else FLOAT),
        label=meta.label,
        group=meta.group,
        default=default,
        integer=integer,
        by_order=by_order,
        soft_min=meta.soft_min,
        soft_max=meta.soft_max,
        description=meta.description,
    )


def _list_kind(path: str, lists: list[list]) -> ParamSpec | None:
    flat = [v for v in lists if v and all(_is_number(c) for c in v)]
    if not flat or len({len(v) for v in flat}) != 1:
        return None
    size = len(flat[0])
    integer = all(isinstance(c, int) for v in flat for c in v)
    meta = uimeta.meta_for(path, float(max(abs(c) for c in flat[0])), integer)
    is_color = size in (3, 4) and path.rsplit(".", 1)[-1].endswith("color")
    return ParamSpec(
        path=path,
        kind=COLOR if is_color else VECTOR,
        label=meta.label,
        group=meta.group,
        default=flat[0],
        size=size,
        integer=integer,
        soft_min=0.0 if is_color else meta.soft_min,
        soft_max=1.0 if is_color else meta.soft_max,
        description=meta.description,
    )


def classify(path: str, values: list[Any], probe: Probe) -> ParamSpec | None:
    """The :class:`ParamSpec` of one field from the values seen, or None if not editable."""
    present = [v for v in values if v is not None]
    if not present:
        return None
    meta = uimeta.meta_for(path)
    if all(isinstance(v, bool) for v in present):
        return ParamSpec(path, BOOL, meta.label, meta.group, present[0])
    if all(isinstance(v, str) for v in present):
        message = probe(path, "\u0000") or ""
        options = enum_options(message) or tuple(dict.fromkeys(present))
        return ParamSpec(path, ENUM, meta.label, meta.group, present[0], options=options)
    if any(is_curve_value(v) for v in present) or all(_is_number(v) for v in present):
        return _numeric_kind(path, present, probe)
    lists = [v for v in present if isinstance(v, list)]
    if len(lists) == len(present):
        return _list_kind(path, lists)
    return None


def _group_name(spec: ParamSpec) -> str:
    parts = spec.path.split(".")
    if len(parts) > 2:
        return uimeta.label_from_key(parts[-2])
    if spec.path in uimeta.TABLE:
        return spec.group
    return "Other"


def _sort_key(spec: ParamSpec, table_order: dict[str, int]) -> tuple:
    focus = spec.path in uimeta.CURVE_FOCUS
    return (not focus, table_order.get(spec.path, len(table_order)), spec.path)


def arrange(specs: Iterable[ParamSpec], species: dict) -> list[Section]:
    """Sections in display order, each with its parameters grouped and sorted."""
    table_order = {path: i for i, path in enumerate(uimeta.TABLE)}
    names = [s for s in uimeta.SECTIONS if s in species]
    names += sorted(
        k
        for k, v in species.items()
        if isinstance(v, dict) and k not in uimeta.SECTIONS and k not in uimeta.HIDDEN_TOP_LEVEL
    )
    sections = {
        name: Section(name, uimeta.SECTIONS.get(name, uimeta.label_from_key(name)))
        for name in names
    }
    for spec in sorted(specs, key=lambda s: _sort_key(s, table_order)):
        section = sections.get(spec.section)
        if section is not None:
            spec.group = _group_name(spec)
            section.groups.setdefault(spec.group, []).append(spec)
    for section in sections.values():
        if "" in section.groups:
            section.groups = {"": section.groups.pop(""), **section.groups}
    return [s for s in sections.values() if s.groups]


def build_schema(species_list: list[dict], probe: Probe) -> list[Section]:
    """Sections and parameters of the species in ``species_list`` (full species dicts)."""
    fields = collect_fields(species_list)
    specs = [spec for path, values in fields.items() if (spec := classify(path, values, probe))]
    union: dict = {}
    for species in species_list:
        for key, value in species.items():
            union.setdefault(key, value)
    return arrange(specs, union)


def order_count(species: dict) -> int:
    """Number of branching orders of a species: ``architecture.max_order + 1``."""
    value = get_path(species, "architecture.max_order")
    return int(value) + 1 if _is_number(value) else 1


def curve_order_count(value: Any) -> int:
    """Number of explicit per-order entries of a curve value."""
    return len(order_entries(value)) if value is not MISSING else 1
