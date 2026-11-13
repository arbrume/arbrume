"""Mesh arrays of a tree in the layout Blender's ``foreach_set`` reads, without Blender.

All parts of the tree go into one mesh: their vertices are concatenated, their triangle
indices offset, and each face gets the material index of its part. Arbrume is Y up and
Blender Z up, so positions and normals are turned by +90 degrees about X,
``(x, y, z) -> (x, -z, y)``: a rotation, which keeps the triangle winding.
"""

from __future__ import annotations

import hashlib
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Any

import numpy as np

PART_ORDER = ("bark", "leaf")
"""Material slot of each part kind; other kinds take the slots after these."""


def y_up_to_z_up(points: np.ndarray, out: np.ndarray | None = None) -> np.ndarray:
    """``(n, 3)`` points or vectors from Y up to Z up, written into ``out`` if given."""
    if out is None:
        out = np.empty(points.shape, dtype=np.float32)
    out[:, 0] = points[:, 0]
    out[:, 1] = points[:, 2]
    np.negative(out[:, 1], out=out[:, 1])
    out[:, 2] = points[:, 1]
    return out


@dataclass
class MeshArrays:
    """Flat arrays of one mesh, ready for ``foreach_set``."""

    positions: np.ndarray
    normals: np.ndarray | None
    corner_verts: np.ndarray
    face_starts: np.ndarray
    material_indices: np.ndarray
    corner_uvs: np.ndarray | None
    colors: np.ndarray | None
    kinds: list[str]
    materials: list[dict]

    @property
    def vertex_count(self) -> int:
        """Number of vertices."""
        return len(self.positions) // 3

    @property
    def face_count(self) -> int:
        """Number of triangles."""
        return len(self.face_starts)


def slot_order(kinds: Sequence[str]) -> list[str]:
    """Distinct part kinds in material slot order: bark, leaf, then any other kind."""
    known = [k for k in PART_ORDER if k in kinds]
    return known + [k for k in dict.fromkeys(kinds) if k not in PART_ORDER]


def _rows(array: Any, count: int, width: int) -> np.ndarray | None:
    data = np.asarray(array)
    if data.ndim != 2 or data.shape[0] != count or data.shape[1] < width:
        return None
    return data[:, :width]


def build_arrays(parts: Sequence[Any]) -> MeshArrays:
    """Concatenated mesh arrays of the tree ``parts`` (``tree.parts``)."""
    kinds = slot_order([part.kind for part in parts])
    counts = [part.vertices.shape[0] for part in parts]
    tris = [part.indices.shape[0] for part in parts]
    total_v, total_t = sum(counts), sum(tris)
    positions = np.empty((total_v, 3), dtype=np.float32)
    normals = np.empty((total_v, 3), dtype=np.float32)
    uvs = np.zeros((total_v, 2), dtype=np.float32)
    colors = np.ones((total_v, 4), dtype=np.float32)
    corners = np.empty((total_t, 3), dtype=np.int32)
    material_indices = np.empty(total_t, dtype=np.int32)
    has_normals = has_uvs = has_colors = False
    v0 = t0 = 0
    for part, nv, nt in zip(parts, counts, tris, strict=True):
        y_up_to_z_up(part.vertices, positions[v0 : v0 + nv])
        part_normals = _rows(part.normals, nv, 3)
        if part_normals is not None:
            y_up_to_z_up(part_normals, normals[v0 : v0 + nv])
            has_normals = True
        else:
            normals[v0 : v0 + nv] = (0.0, 0.0, 1.0)
        part_uvs = _rows(part.uvs, nv, 2)
        if part_uvs is not None:
            uvs[v0 : v0 + nv] = part_uvs
            has_uvs = True
        part_colors = _rows(part.colors, nv, 4)
        if part_colors is not None:
            colors[v0 : v0 + nv] = part_colors
            has_colors = True
        np.add(part.indices, v0, out=corners[t0 : t0 + nt], casting="unsafe")
        material_indices[t0 : t0 + nt] = kinds.index(part.kind)
        v0 += nv
        t0 += nt
    flat_corners = corners.reshape(-1)
    by_kind = {part.kind: dict(part.material) for part in parts}
    return MeshArrays(
        positions=positions.reshape(-1),
        normals=normals if has_normals else None,
        corner_verts=flat_corners,
        face_starts=np.arange(0, 3 * total_t, 3, dtype=np.int32),
        material_indices=material_indices,
        corner_uvs=uvs[flat_corners].reshape(-1) if has_uvs else None,
        colors=colors.reshape(-1) if has_colors else None,
        kinds=kinds,
        materials=[by_kind[k] for k in kinds],
    )


def material_key(species_name: str, kind: str, material: dict) -> str:
    """Stable name of the Blender material for one part material of one species."""
    color = material.get("base_color") or [1.0, 1.0, 1.0, 1.0]
    values = [*color[:4], material.get("roughness", 0.5), float(bool(material.get("double_sided")))]
    text = ",".join(f"{float(v):.4f}" for v in values)
    digest = hashlib.sha1(text.encode()).hexdigest()[:6]
    return f"Arbrume {species_name[:32]} {kind} {digest}"
