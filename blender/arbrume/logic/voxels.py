"""Voxelizing triangle meshes into the frame of a plant, without Blender.

An envelope or obstacle object is read as triangles, moved into the frame of the plant
(the plant at the origin, Arbrume Y up) and turned into a grid of cubic voxels for
``arbrume.voxel_mask``:

* the **solid** of a closed mesh is filled by parity: a ray is cast along Y through the
  centre of every column of voxels, and a voxel is inside when an odd number of surface
  crossings lie below its centre;
* the **shell** marks every voxel a triangle passes through, sampled at half a voxel
  spacing, so thin or open meshes (a plane used as a wall) still block.

The voxel edge comes from the species: half its shortest internode, times the plant
scale (:func:`auto_cell`), so that a shoot cannot step over a wall one voxel thick. The
grid of one object is limited to :data:`MAX_VOXELS` voxels, the edge growing to fit.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import Any

import numpy as np

MAX_VOXELS = 8_000_000
"""Largest number of voxels in the grid of one object."""
MIN_CELL = 0.005
"""Smallest voxel edge, in meters."""
JITTER = np.array([0.5377, 0.0, 0.7913])
"""Offset of the column rays, in voxel edges times 1e-4, off the edges of the mesh."""


@dataclass(frozen=True)
class Grid:
    """A voxelized volume: corner, voxel edge, voxel counts along X, Y, Z and the mask."""

    origin: tuple[float, float, float]
    cell: float
    shape: tuple[int, int, int]
    mask: np.ndarray

    @property
    def filled(self) -> int:
        """Number of filled voxels."""
        return int(self.mask.sum())


def blender_to_plant(plant_matrix: np.ndarray, object_matrix: np.ndarray) -> np.ndarray:
    """The 4x4 matrix from the local space of an object to the frame of a plant (Y up):
    the inverse of the plant world matrix, times the object world matrix, then Blender Z up
    turned to Arbrume Y up, ``(x, y, z) -> (x, z, -y)``."""
    z_up_to_y_up = np.array(
        [[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [0, 0, 0, 1]], dtype=np.float64
    )
    relative = np.linalg.inv(np.asarray(plant_matrix, dtype=np.float64)) @ np.asarray(
        object_matrix, dtype=np.float64
    )
    return z_up_to_y_up @ relative


def transform(points: np.ndarray, matrix: np.ndarray) -> np.ndarray:
    """``(n, 3)`` points moved by a 4x4 matrix."""
    points = np.asarray(points, dtype=np.float64).reshape(-1, 3)
    return points @ matrix[:3, :3].T + matrix[:3, 3]


def geometry_key(coords: np.ndarray, triangles: np.ndarray, matrix: np.ndarray, *extra) -> str:
    """Hash of the geometry, the transform and the voxelization settings of an object."""
    digest = hashlib.sha1()
    for array in (coords, triangles, matrix):
        data = np.ascontiguousarray(array)
        digest.update(str(data.dtype).encode() + str(data.shape).encode())
        digest.update(data.tobytes())
    digest.update(repr(extra).encode())
    return digest.hexdigest()


def internode_length(growth: dict) -> float | None:
    """Shortest internode length of a species ``growth`` table, whether a number, a
    ``by_order`` list or curves of ``[t, value]`` points; ``None`` when it gives none."""
    values: list[float] = []

    def collect(value: Any) -> None:
        if isinstance(value, bool):
            return
        if isinstance(value, int | float):
            values.append(float(value))
        elif isinstance(value, dict):
            collect(value.get("by_order"))
        elif isinstance(value, list):
            for item in value:
                is_point = isinstance(item, list) and len(item) == 2
                collect(item[1] if is_point and not isinstance(item[0], list) else item)

    collect(growth.get("internode_length"))
    positive = [value for value in values if value > 0.0]
    return min(positive) if positive else None


def auto_cell(species: dict, scale: float) -> float:
    """Voxel edge for a plant: half its shortest internode, times its scale."""
    internode = internode_length(species.get("growth", {})) or 0.1
    return max(MIN_CELL, 0.5 * internode * scale)


def is_self_organizing(species: dict) -> bool:
    """Whether a species grows in the self-organizing mode, the one surroundings act on."""
    return species.get("growth", {}).get("mode") == "self_organizing"


def climbs(species: dict) -> bool:
    """Whether a species has a ``[climbing]`` table: its shoots hold the blocking
    obstacles, in either growth mode."""
    return bool(species.get("climbing"))


def _layout(points: np.ndarray, cell: float, pad: int) -> tuple[np.ndarray, float, np.ndarray]:
    """Corner, voxel edge and voxel counts of a grid around ``points``, ``pad`` voxels
    wider on every side, the edge grown so the grid holds at most ``MAX_VOXELS``."""
    low, high = points.min(axis=0), points.max(axis=0)
    size = np.maximum(high - low, 1e-6)
    volume = float(np.prod(size + 2 * pad * cell))
    if volume / cell**3 > MAX_VOXELS:
        cell = (volume / MAX_VOXELS) ** (1.0 / 3.0) * 1.01
    count = np.ceil(size / cell).astype(np.int64) + 2 * pad
    return low - pad * cell, cell, count


def _crossings(tris: np.ndarray, origin: np.ndarray, cell: float, count: np.ndarray):
    """Columns ``(x, z)`` and heights where the vertical rays through the column centres
    cross the triangles ``(n, 3, 3)``."""
    shift = JITTER * 1e-4 * cell
    xz = tris[:, :, [0, 2]]
    lo = np.floor((xz.min(axis=1) - origin[[0, 2]] - shift[[0, 2]]) / cell - 0.5).astype(np.int64)
    hi = np.ceil((xz.max(axis=1) - origin[[0, 2]] - shift[[0, 2]]) / cell - 0.5).astype(np.int64)
    lo = np.clip(lo, 0, count[[0, 2]] - 1)
    hi = np.clip(hi, 0, count[[0, 2]] - 1)
    spans = hi - lo + 1
    per_tri = spans[:, 0] * spans[:, 1]
    owner = np.repeat(np.arange(len(tris)), per_tri)
    local = np.arange(len(owner)) - np.repeat(np.cumsum(per_tri) - per_tri, per_tri)
    ix = lo[owner, 0] + local // spans[owner, 1]
    iz = lo[owner, 1] + local % spans[owner, 1]
    px = origin[0] + (ix + 0.5) * cell + shift[0]
    pz = origin[2] + (iz + 0.5) * cell + shift[2]
    a, b, c = tris[owner, 0], tris[owner, 1], tris[owner, 2]
    e1x, e1z, e2x, e2z = b[:, 0] - a[:, 0], b[:, 2] - a[:, 2], c[:, 0] - a[:, 0], c[:, 2] - a[:, 2]
    det = e1x * e2z - e2x * e1z
    safe = np.where(det == 0.0, 1.0, det)
    dx, dz = px - a[:, 0], pz - a[:, 2]
    u = (dx * e2z - e2x * dz) / safe
    v = (e1x * dz - dx * e1z) / safe
    hit = (det != 0.0) & (u >= 0.0) & (v >= 0.0) & (u + v <= 1.0)
    y = a[:, 1] + u * (b[:, 1] - a[:, 1]) + v * (c[:, 1] - a[:, 1])
    return ix[hit], iz[hit], y[hit]


def _solid(tris: np.ndarray, origin: np.ndarray, cell: float, count: np.ndarray) -> np.ndarray:
    """Voxels inside a closed mesh, by the parity of the crossings below their centres."""
    nx, ny, nz = (int(n) for n in count)
    ix, iz, y = _crossings(tris, origin, cell, count)
    first_above = np.ceil((y - origin[1]) / cell - 0.5).astype(np.int64)
    toggles = np.zeros((nx, nz, ny + 1), dtype=np.int32)
    np.add.at(toggles, (ix, iz, np.clip(first_above, 0, ny)), 1)
    inside = np.cumsum(toggles, axis=2)[:, :, :ny] % 2 == 1
    return np.ascontiguousarray(inside.transpose(0, 2, 1))


def _shell(tris: np.ndarray, origin: np.ndarray, cell: float, count: np.ndarray) -> np.ndarray:
    """Voxels the triangles pass through, sampled at half a voxel spacing."""
    mask = np.zeros(tuple(int(n) for n in count), dtype=bool)
    edges = np.maximum(
        np.linalg.norm(tris[:, 1] - tris[:, 0], axis=1),
        np.linalg.norm(tris[:, 2] - tris[:, 0], axis=1),
    )
    steps = np.maximum(1, np.ceil(2.0 * edges / cell).astype(np.int64))
    for step in np.unique(steps):
        group = tris[steps == step]
        i, j = np.meshgrid(np.arange(step + 1), np.arange(step + 1), indexing="ij")
        keep = i + j <= step
        u, v = i[keep] / step, j[keep] / step
        a, b, c = group[:, None, 0], group[:, None, 1], group[:, None, 2]
        points = a + u[None, :, None] * (b - a) + v[None, :, None] * (c - a)
        index = np.floor((points.reshape(-1, 3) - origin) / cell).astype(np.int64)
        index = np.clip(index, 0, count - 1)
        mask[index[:, 0], index[:, 1], index[:, 2]] = True
    return mask


def voxelize(tris: np.ndarray, cell: float, solid: bool = True, shell: bool = True) -> Grid:
    """The voxels of the triangles ``(n, 3, 3)`` (meters, plant frame): the solid of a
    closed mesh, its shell, or both, in a grid one voxel wider than the mesh."""
    tris = np.asarray(tris, dtype=np.float64).reshape(-1, 3, 3)
    if len(tris) == 0:
        raise ValueError("the mesh has no faces")
    origin, cell, count = _layout(tris.reshape(-1, 3), cell, 1)
    mask = np.zeros(tuple(int(n) for n in count), dtype=bool)
    if solid:
        mask |= _solid(tris, origin, cell, count)
    if shell:
        mask |= _shell(tris, origin, cell, count)
    return Grid(tuple(float(v) for v in origin), float(cell), tuple(int(n) for n in count), mask)


def with_ground_column(grid: Grid) -> Grid:
    """``grid`` grown down to the ground (Y = 0) with a column of voxels around the plant
    axis, from the ground up to the filled voxels, so that a trunk can rise into a crown
    envelope that floats above the ground. Unchanged when the axis misses the grid."""
    nx, ny, nz = grid.shape
    cell = grid.cell
    below = max(0, int(np.ceil((grid.origin[1] + cell) / cell)))
    origin = (grid.origin[0], grid.origin[1] - below * cell, grid.origin[2])
    mask = np.zeros((nx, ny + below, nz), dtype=bool)
    mask[:, below:, :] = grid.mask
    cx = int(np.floor(-origin[0] / cell))
    cz = int(np.floor(-origin[2] / cell))
    if not (0 <= cx < nx and 0 <= cz < nz):
        return grid
    xs, zs = slice(max(cx - 1, 0), cx + 2), slice(max(cz - 1, 0), cz + 2)
    column = mask[xs, :, zs].any(axis=(0, 2))
    if not column.any():
        return grid
    top = int(np.argmax(column))
    mask[xs, :top, zs] = True
    return Grid(origin, cell, (nx, ny + below, nz), mask)
