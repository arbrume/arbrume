"""Brush strokes of a plant, without Blender: projection, resampling and the guides text.

A plant stores its strokes as the JSON text of the ``guides`` argument of
``arbrume.generate`` (see ``docs/brush.md``): points in meters in the frame of the plant
(the plant at the origin, Arbrume Y up), so that strokes move, turn and duplicate with
their plant object.

A stroke drawn in the 3D view is projected onto a *drawing plane* parallel to the screen
through an anchor: the point of the plant (or of another surface) under the first point
of the stroke, or the origin of the plant. In a side view this is the vertical plane
through the plant of TreeSketch (Longay et al. 2012). Points closer than a spacing to the
previous one are skipped while drawing, and a finished stroke is resampled evenly.
"""

from __future__ import annotations

import json
import math
from typing import Any

import numpy as np

from .voxels import blender_to_plant, transform

KINDS = {"CROWN": "crown", "LIMB": "limb", "ERASE": "erase"}
"""Brush modes and the stroke kind each one draws."""
DECIMALS = 4
"""Decimals kept for stroke coordinates in the stored text (a tenth of a millimeter)."""
MAX_POINTS = 256
"""Largest number of points of one stored stroke."""


def world_to_plant(plant_matrix: np.ndarray, points: np.ndarray) -> np.ndarray:
    """World points (Blender, Z up) in the frame of a plant with world matrix
    ``plant_matrix`` (Arbrume, Y up)."""
    return transform(points, blender_to_plant(plant_matrix, np.eye(4)))


def plant_to_world(plant_matrix: np.ndarray, points: np.ndarray) -> np.ndarray:
    """Points in the frame of a plant back in world space."""
    inverse = np.linalg.inv(blender_to_plant(plant_matrix, np.eye(4)))
    return transform(points, inverse)


def plant_scale(plant_matrix: np.ndarray) -> float:
    """Mean scale of a plant's world matrix: world meters per meter of the plant frame."""
    columns = np.asarray(plant_matrix, dtype=np.float64)[:3, :3]
    return float(np.cbrt(abs(np.linalg.det(columns)))) or 1.0


def ray_plane(origin, direction, point, normal) -> np.ndarray | None:
    """Where the ray from ``origin`` along ``direction`` meets the plane through ``point``
    with ``normal``; ``None`` when the ray is parallel to the plane or points away."""
    origin, direction = np.asarray(origin, float), np.asarray(direction, float)
    point, normal = np.asarray(point, float), np.asarray(normal, float)
    facing = float(direction @ normal)
    if abs(facing) < 1e-9:
        return None
    t = float((point - origin) @ normal) / facing
    return origin + t * direction if t >= 0.0 else None


def drawing_plane(view_direction, anchor) -> tuple[np.ndarray, np.ndarray]:
    """The plane strokes are projected on: through ``anchor``, facing the viewer."""
    normal = np.asarray(view_direction, float)
    length = float(np.linalg.norm(normal))
    if length == 0.0:
        raise ValueError("the view direction is zero")
    return np.asarray(anchor, float), normal / length


def should_append(points: list, point, spacing: float) -> bool:
    """Whether ``point`` is at least ``spacing`` from the last point of ``points``."""
    if not points:
        return True
    return float(np.linalg.norm(np.asarray(point) - np.asarray(points[-1]))) >= spacing


def resample(points, spacing: float, limit: int = MAX_POINTS) -> np.ndarray:
    """``points`` resampled every ``spacing`` of arc length (ends kept), with at most
    ``limit`` points: the spacing grows for a long stroke. One point stays one point."""
    points = np.asarray(points, dtype=np.float64).reshape(-1, 3)
    if len(points) < 2:
        return points
    steps = np.linalg.norm(np.diff(points, axis=0), axis=1)
    arcs = np.concatenate([[0.0], np.cumsum(steps)])
    length = float(arcs[-1])
    if length == 0.0:
        return points[:1]
    count = min(limit - 1, max(1, math.ceil(length / max(spacing, 1e-9))))
    samples = np.linspace(0.0, length, count + 1)
    return np.stack([np.interp(samples, arcs, points[:, axis]) for axis in range(3)], axis=1)


def swept(points, direction, depth: float, spacing: float) -> list[np.ndarray]:
    """Segments of length ``2 * depth`` along ``direction`` through ``points`` resampled
    every ``spacing``: the lines of an erase stroke that cuts through everything under the
    brush along the view, not only near the drawing plane."""
    unit = np.asarray(direction, dtype=np.float64)
    unit = unit / np.linalg.norm(unit)
    return [np.stack([p - unit * depth, p + unit * depth]) for p in resample(points, spacing)]


def load(text: str) -> dict:
    """The guides of a plant from their stored text; empty guides for empty text."""
    if not text.strip():
        return {"strokes": [], "freezes": []}
    guides = json.loads(text)
    if not isinstance(guides, dict):
        raise ValueError("guides text is not a JSON object")
    guides.setdefault("strokes", [])
    guides.setdefault("freezes", [])
    return guides


def argument(text: str) -> dict | None:
    """The ``guides`` argument of ``arbrume.generate`` for a plant's stored text: ``None``
    when the plant has no strokes, so that it is generated exactly as before."""
    guides = load(text)
    return guides if guides["strokes"] or guides["freezes"] else None


def dump(guides: dict) -> str:
    """The stored text of ``guides``: compact JSON, ``""`` when there is nothing."""
    if not guides.get("strokes") and not guides.get("freezes"):
        return ""
    return json.dumps(guides, separators=(",", ":"), sort_keys=True)


def current_cycle(guides: dict) -> int:
    """The cycle new strokes act from: the last freeze, or 0."""
    freezes = guides.get("freezes") or []
    return int(freezes[-1]) if freezes else 0


def make_stroke(mode: str, points, radius: float, options: dict[str, float]) -> dict:
    """A stroke of brush mode ``mode`` through ``points`` (plant frame), rounded for
    storage. ``options`` holds the numbers the kind uses (density, strength, follow,
    reach); the others are left out."""
    kind = KINDS[mode]
    used = {
        "crown": ("density", "strength", "reach"),
        "limb": ("density", "strength", "follow", "reach"),
        "erase": (),
    }[kind]
    stroke: dict[str, Any] = {
        "kind": kind,
        "points": [[round(float(v), DECIMALS) for v in p] for p in np.asarray(points)],
        "radius": round(float(radius), DECIMALS),
    }
    for name in used:
        if name in options:
            stroke[name] = round(float(options[name]), DECIMALS)
    return stroke


def with_stroke(guides: dict, stroke: dict) -> dict:
    """``guides`` with ``stroke`` added, acting from the current cycle."""
    stroke = dict(stroke)
    cycle = current_cycle(guides)
    if cycle:
        stroke["cycle"] = cycle
    return {**guides, "strokes": [*guides.get("strokes", []), stroke]}


def with_freeze(guides: dict, age: int) -> dict:
    """``guides`` frozen at ``age``; unchanged when already frozen at that age or later."""
    freezes = list(guides.get("freezes") or [])
    if freezes and freezes[-1] >= age:
        return dict(guides)
    return {**guides, "freezes": [*freezes, int(age)]}


def without_last_freeze(guides: dict) -> tuple[dict, int | None]:
    """``guides`` without their last freeze and the strokes drawn after it, and the age
    of that freeze (``None`` when there is none)."""
    freezes = list(guides.get("freezes") or [])
    if not freezes:
        return dict(guides), None
    last = freezes.pop()
    strokes = [s for s in guides.get("strokes", []) if int(s.get("cycle", 0)) < last]
    return {**guides, "strokes": strokes, "freezes": freezes}, last


def counts(guides: dict) -> dict[str, int]:
    """Number of strokes of each kind."""
    result = dict.fromkeys(KINDS.values(), 0)
    for stroke in guides.get("strokes", []):
        result[stroke.get("kind", "")] = result.get(stroke.get("kind", ""), 0) + 1
    return result


def _largest(value: Any) -> float:
    """Largest number in a species value: a number, a curve, or curves per order."""
    if isinstance(value, bool):
        return 0.0
    if isinstance(value, int | float):
        return float(value)
    if isinstance(value, dict):
        return max((_largest(v) for v in value.values()), default=0.0)
    if isinstance(value, list):
        return max((_largest(v) for v in value), default=0.0)
    return 0.0


def _order(value: Any, order: int) -> Any:
    """The curve of ``order`` in a value given per order, or the value itself."""
    if isinstance(value, dict) and isinstance(value.get("by_order"), list):
        orders = value["by_order"]
        return orders[min(order, len(orders) - 1)] if orders else 0.0
    return value


def features(species: dict) -> np.ndarray:
    """Numbers that describe the habit of a species, to compare species: the logarithms of
    the trunk height, of the branch reach and of the number of stems, the highest order
    over 4, and 1 for branches in tiers (a rhythmic branching pattern, as in conifers)."""
    growth = species.get("growth", {})
    lengths = growth.get("axis_length", 1.0)
    height = max(_largest(_order(lengths, 0)), 1e-3)
    reach = max(_largest(_order(lengths, 1)), 1e-3)
    stems = float(growth.get("stems", 1) or 1)
    architecture = species.get("architecture", {})
    order = float(architecture.get("max_order", 3) or 3)
    tiers = 1.0 if architecture.get("branching_pattern") == "rhythmic" else 0.0
    return np.array([math.log(height), math.log(reach), math.log(stems), order / 4.0, tiers])


def is_self_organizing(species: dict) -> bool:
    """Whether a species grows in the self-organizing mode, which strokes steer."""
    return species.get("growth", {}).get("mode") == "self_organizing"


def nearest_self_organizing(species: dict, presets: dict[str, dict]) -> str | None:
    """The self-organizing preset closest in habit (:func:`features`) to ``species``,
    climbing plants left out; ``None`` when there is none."""
    target = features(species)
    best: tuple[float, str] | None = None
    for name in sorted(presets):
        candidate = presets[name]
        if not is_self_organizing(candidate) or candidate.get("climbing"):
            continue
        distance = float(np.linalg.norm(features(candidate) - target))
        if best is None or distance < best[0]:
            best = (distance, name)
    return None if best is None else best[1]
