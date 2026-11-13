"""Species values, overrides and curves as plain Python data.

A species is the JSON form of the Rust ``Species`` (``tree.metadata["species"]``): nested
dicts of numbers, strings, booleans, lists and curves. Overrides are a flat dict from a
dotted path (``"growth.gnarl"``) to the value that replaces the whole parameter, exactly
as ``arbrume.generate(..., overrides=...)`` applies them.

A curve is a number, a list of ``[x, y]`` points, ``{"points": [...], "interp": ...}``, or
``{"by_order": [curve, ...]}`` where orders past the end of the list use the last entry.
"""

from __future__ import annotations

import copy
import json
import math
from dataclasses import dataclass
from itertools import pairwise
from typing import Any

import numpy as np

MISSING = object()
"""Returned by :func:`get_path` for a path that the species does not have."""

REL_TOLERANCE = 1e-6
"""Relative tolerance of value comparisons: the species stores ``f32`` numbers."""


def f32_text(value: float) -> str:
    """Shortest decimal text that reads back as the same ``f32`` as ``value``."""
    target = np.float32(value)
    if not math.isfinite(float(target)):
        raise ValueError(f"not a finite number: {value}")
    text = repr(float(target))
    for digits in range(6, 10):
        candidate = f"{float(target):.{digits}g}"
        if np.float32(float(candidate)) == target:
            text = candidate
            break
    if not any(mark in text for mark in ".en"):
        text += ".0"
    return text


def clean_float(value: float) -> float:
    """``value`` rounded to the shortest decimal of its ``f32``, so that 0.13 stays 0.13."""
    return float(f32_text(value))


def clean_value(value: Any) -> Any:
    """``value`` with every float cleaned by :func:`clean_float`, recursively."""
    if isinstance(value, bool) or value is None or isinstance(value, int | str):
        return value
    if isinstance(value, float):
        return clean_float(value)
    if isinstance(value, dict):
        return {key: clean_value(item) for key, item in value.items()}
    if isinstance(value, list | tuple):
        return [clean_value(item) for item in value]
    return value


def values_equal(a: Any, b: Any) -> bool:
    """Whether two species values are equal, numbers compared at ``f32`` precision."""
    if isinstance(a, bool) or isinstance(b, bool):
        return a is b
    if isinstance(a, int | float) and isinstance(b, int | float):
        return math.isclose(a, b, rel_tol=REL_TOLERANCE, abs_tol=1e-9)
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(values_equal(a[k], b[k]) for k in a)
    if isinstance(a, list | tuple) and isinstance(b, list | tuple):
        return len(a) == len(b) and all(values_equal(x, y) for x, y in zip(a, b, strict=True))
    return a == b


def get_path(species: dict, path: str) -> Any:
    """Value at the dotted ``path`` of ``species``, or :data:`MISSING`."""
    node: Any = species
    for segment in path.split("."):
        if not isinstance(node, dict) or segment not in node:
            return MISSING
        node = node[segment]
    return node


def _set_path(species: dict, path: str, value: Any) -> None:
    *parents, leaf = path.split(".")
    node = species
    for segment in parents:
        child = node.get(segment)
        if not isinstance(child, dict):
            child = {}
            node[segment] = child
        node = child
    node[leaf] = copy.deepcopy(value)


def apply_overrides(species: dict, overrides: dict) -> dict:
    """A copy of ``species`` with ``overrides`` applied as the generator applies them.

    A dotted key replaces the whole value at its path. A top-level key without a dot whose
    value is a table sets each entry of that table inside the existing section.
    """
    result = copy.deepcopy(species)
    for key, value in overrides.items():
        section = result.get(key)
        if "." not in key and isinstance(section, dict) and isinstance(value, dict):
            for inner, item in value.items():
                _set_path(result, f"{key}.{inner}", item)
        else:
            _set_path(result, key, value)
    return result


def load_overrides(text: str) -> dict:
    """Overrides stored as JSON text; an empty dict for empty or invalid text."""
    try:
        value = json.loads(text) if text else {}
    except (TypeError, ValueError):
        return {}
    return value if isinstance(value, dict) else {}


def dump_overrides(overrides: dict) -> str:
    """JSON text of ``overrides``, keys sorted so equal dicts give equal text."""
    return json.dumps(clean_value(overrides), sort_keys=True, separators=(",", ":"))


def with_override(overrides: dict, base: dict, path: str, value: Any) -> dict:
    """``overrides`` with ``path`` set to ``value``, or removed when ``value`` is the base."""
    result = dict(overrides)
    if values_equal(value, get_path(base, path)):
        result.pop(path, None)
    else:
        result[path] = clean_value(value)
    return result


def without_override(overrides: dict, path: str) -> dict:
    """``overrides`` without ``path``, nor any override inside or above it."""
    prefix = path + "."
    return {
        key: value
        for key, value in overrides.items()
        if key != path and not key.startswith(prefix) and not path.startswith(key + ".")
    }


@dataclass(frozen=True)
class CurvePoints:
    """Control points of one curve, sorted by ``x``, and whether they join smoothly."""

    points: tuple[tuple[float, float], ...]
    smooth: bool = False

    @staticmethod
    def flat(value: float) -> CurvePoints:
        """The constant ``value`` as a straight two-point curve."""
        return CurvePoints(((0.0, value), (1.0, value)))

    def normalized(self) -> CurvePoints:
        """Points clamped to ``x`` in [0, 1], sorted, with at most one point per ``x``."""
        by_x: dict[float, float] = {}
        for x, y in self.points:
            by_x[clean_float(min(1.0, max(0.0, float(x))))] = clean_float(float(y))
        points = tuple(sorted(by_x.items()))
        if len(points) == 1:
            points = ((0.0, points[0][1]), (1.0, points[0][1]))
        return CurvePoints(points, self.smooth)

    def mean(self) -> float:
        """Mean of the curve over [0, 1] with linear joins, a constant that stands for it."""
        pts = self.normalized().points
        area = pts[0][1] * pts[0][0] + pts[-1][1] * (1.0 - pts[-1][0])
        for (x0, y0), (x1, y1) in pairwise(pts):
            area += 0.5 * (y0 + y1) * (x1 - x0)
        return clean_float(area)

    def close_to(self, other: CurvePoints) -> bool:
        """Same points within ``f32`` precision and the same interpolation."""
        return self.smooth == other.smooth and values_equal(self.points, other.points)


def _is_point_list(value: Any) -> bool:
    return (
        isinstance(value, list)
        and len(value) > 0
        and all(
            isinstance(p, list | tuple)
            and len(p) == 2
            and all(isinstance(c, int | float) and not isinstance(c, bool) for c in p)
            for p in value
        )
    )


def is_curve_value(value: Any) -> bool:
    """Whether ``value`` has the shape of a curve other than a bare number."""
    if _is_point_list(value):
        return True
    return isinstance(value, dict) and ("by_order" in value or "points" in value)


def order_entries(value: Any) -> list:
    """The per-order entries of a curve; a curve without orders is one entry."""
    if isinstance(value, dict) and isinstance(value.get("by_order"), list):
        return list(value["by_order"]) or [0.0]
    return [value]


def entry_at(value: Any, order: int) -> Any:
    """The curve used at branching ``order``: orders past the end use the last entry."""
    entries = order_entries(value)
    return entries[min(max(order, 0), len(entries) - 1)]


def entry_points(entry: Any) -> CurvePoints | None:
    """Control points of a single curve, or None for a constant."""
    if _is_point_list(entry):
        return CurvePoints(tuple((float(x), float(y)) for x, y in entry))
    if isinstance(entry, dict) and _is_point_list(entry.get("points")):
        points = tuple((float(x), float(y)) for x, y in entry["points"])
        return CurvePoints(points, entry.get("interp") == "smooth")
    return None


def entry_constant(entry: Any) -> float | None:
    """The value of a constant curve, or None for a curve with points."""
    if isinstance(entry, int | float) and not isinstance(entry, bool):
        return float(entry)
    return None


def entry_display_value(entry: Any) -> float:
    """A number standing for a single curve: its constant, or the mean of its points."""
    constant = entry_constant(entry)
    if constant is not None:
        return constant
    points = entry_points(entry)
    return points.mean() if points is not None else 0.0


def points_entry(curve: CurvePoints) -> Any:
    """The species value of a single curve given by its points."""
    pts = [[x, y] for x, y in curve.normalized().points]
    return {"points": pts, "interp": "smooth"} if curve.smooth else pts


def with_entry(value: Any, order: int, entry: Any, order_count: int) -> Any:
    """Curve ``value`` with the curve of ``order`` replaced by ``entry``.

    The list of orders is first extended to ``order_count`` entries (the orders the plant
    has), so that the other orders keep their curves, then trailing repeats are dropped,
    since the last entry stands for every higher order.
    """
    entries = order_entries(value)
    size = max(order_count, order + 1, len(entries))
    while len(entries) < size:
        entries.append(copy.deepcopy(entries[-1]))
    entries[order] = entry
    while len(entries) > 1 and values_equal(entries[-1], entries[-2]):
        entries.pop()
    return clean_value(entries[0] if len(entries) == 1 else {"by_order": entries})
