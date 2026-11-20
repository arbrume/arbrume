"""Forest helpers without Blender: the species mix, the terrain grid, the config and the
conversion of the placements to Blender's frame.

Arbrume is Y up and Blender Z up; a point ``(x, y, z)`` of Arbrume is ``(x, -z, y)`` in
Blender (see ``meshdata``). The terrain grid of the config runs along Arbrume ``x`` and
``z``, so its rows go along Blender ``-y``: row 0 is the largest Blender ``y``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import numpy as np

from .meshdata import y_up_to_z_up

DEFAULT_MIX = "excurrent_conifer:90,forest_grown_broadleaf:50"
"""Species mix of a new forest: preset or pack species and trees per hectare."""


def parse_mix(text: str) -> list[tuple[str, float]]:
    """``"species:density, species:density"`` as pairs; a missing density is 100."""
    mix = []
    for raw in text.split(","):
        item = raw.strip()
        if not item:
            continue
        name, _, density = item.rpartition(":") if ":" in item else (item, "", "100")
        try:
            value = float(density)
        except ValueError as error:
            raise ValueError(f"density of `{name}` is not a number: {density!r}") from error
        if value < 0.0:
            raise ValueError(f"density of `{name}` is negative")
        mix.append((name.strip(), value))
    if not mix:
        raise ValueError("the species mix is empty")
    return mix


@dataclass(frozen=True)
class Grid:
    """A terrain grid over a rectangle of the Blender ground plane."""

    origin: tuple[float, float]
    """Arbrume ``(x, z)`` of sample ``(0, 0)``."""
    size: tuple[float, float]
    """Extent along Arbrume ``x`` and ``z``."""
    columns: int
    rows: int

    def blender_xy(self) -> tuple[np.ndarray, np.ndarray]:
        """Blender ``x`` and ``y`` of every sample, row-major, as two flat arrays."""
        xs = self.origin[0] + np.linspace(0.0, self.size[0], self.columns)
        zs = self.origin[1] + np.linspace(0.0, self.size[1], self.rows)
        gx, gz = np.meshgrid(xs, zs)
        return gx.reshape(-1), -gz.reshape(-1)


def grid_over(low: tuple[float, float], high: tuple[float, float], resolution: float) -> Grid:
    """The grid over the Blender rectangle ``low``-``high`` (``x``, ``y``) with samples about
    ``resolution`` apart."""
    width, depth = high[0] - low[0], high[1] - low[1]
    if width <= 0.0 or depth <= 0.0:
        raise ValueError("the terrain has no extent in x and y")
    if resolution <= 0.0:
        raise ValueError("the resolution must be positive")
    columns = max(2, round(width / resolution) + 1)
    rows = max(2, round(depth / resolution) + 1)
    return Grid(origin=(low[0], -high[1]), size=(width, depth), columns=columns, rows=rows)


def forest_config(
    grid: Grid,
    heights: np.ndarray,
    mix: list[tuple[str, float]],
    options: dict[str, Any],
) -> dict[str, Any]:
    """The config of ``arbrume.forest`` for a sampled terrain.

    ``heights`` are the ground heights of the samples, row-major, ``nan`` where the
    terrain has no ground; those are filled from their neighbours and excluded. ``options``
    gives ``seed``, ``density`` (a factor on every density), ``variants``, ``lods``,
    ``billboard`` and ``textures``.
    """
    heights = np.asarray(heights, dtype=np.float64).reshape(grid.rows, grid.columns)
    missing = ~np.isfinite(heights)
    if missing.all():
        raise ValueError("no sample of the terrain grid hit the terrain")
    filled = fill_missing(heights)
    config: dict[str, Any] = {
        "name": "forest",
        "seed": int(options.get("seed", 0)),
        "terrain": {
            "size": list(grid.size),
            "origin": list(grid.origin),
            "grid": {
                "columns": grid.columns,
                "rows": grid.rows,
                "heights": filled.reshape(-1).tolist(),
            },
        },
        "species": [
            {
                "species": name,
                "density": density * float(options.get("density", 1.0)),
                "variants": int(options.get("variants", 3)),
                "lean": 3.0,
                "tint": 0.08,
            }
            for name, density in mix
        ],
        "library": {
            "lods": int(options.get("lods", 2)),
            "billboard": options.get("billboard", "cloud"),
            "textures": bool(options.get("textures", True)),
        },
    }
    if missing.any():
        values = missing.astype(np.float32).reshape(-1).tolist()
        mask = {"columns": grid.columns, "rows": grid.rows, "values": values}
        config["exclude"] = [{"mask_grid": mask}]
    return config


def fill_missing(heights: np.ndarray) -> np.ndarray:
    """``heights`` with every ``nan`` replaced by the mean of its valid neighbours, pass
    after pass, so that the ground stays continuous where the terrain has no sample."""
    filled = heights.copy()
    while True:
        missing = ~np.isfinite(filled)
        if not missing.any():
            return filled
        valid = np.pad(np.where(missing, 0.0, filled), 1)
        weight = np.pad((~missing).astype(np.float64), 1)
        rows, columns = filled.shape
        total, count = np.zeros_like(filled), np.zeros_like(filled)
        for dr, dc in ((0, 1), (2, 1), (1, 0), (1, 2)):
            total += valid[dr : dr + rows, dc : dc + columns]
            count += weight[dr : dr + rows, dc : dc + columns]
        grow = missing & (count > 0)
        filled[grow] = total[grow] / count[grow]


def blender_points(positions: np.ndarray) -> np.ndarray:
    """Arbrume positions ``(n, 3)`` in Blender's frame."""
    return y_up_to_z_up(np.ascontiguousarray(positions, dtype=np.float32))


def blender_quaternions(rotations: np.ndarray) -> np.ndarray:
    """Arbrume rotations ``(n, 4)``, ``(x, y, z, w)``, as Blender quaternions ``(w, x, y, z)``
    in Blender's frame: the axis turns like a vector, ``(x, -z, y)``."""
    rotations = np.asarray(rotations, dtype=np.float32)
    out = np.empty(rotations.shape, dtype=np.float32)
    out[:, 0] = rotations[:, 3]
    out[:, 1] = rotations[:, 0]
    out[:, 2] = -rotations[:, 2]
    out[:, 3] = rotations[:, 1]
    return out


def instance_indices(species: np.ndarray, variants: np.ndarray, counts: list[int]) -> np.ndarray:
    """Index of the variant of every tree in the list of all variants, species after
    species: what *Instance on Points* picks among the children of the library."""
    offsets = np.concatenate([[0], np.cumsum(counts)[:-1]]).astype(np.int32)
    return offsets[np.asarray(species, dtype=np.int64)] + np.asarray(variants, dtype=np.int32)
