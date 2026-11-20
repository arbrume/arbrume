"""Crown envelope and obstacle objects of a plant, voxelized for the generator.

A plant may name an envelope object and a list of obstacle objects (mesh objects of the
scene). On regeneration each is evaluated (modifiers applied), moved into the frame of
the plant and voxelized (:mod:`.logic.voxels`) into an ``arbrume.voxel_mask``; the masks
go to ``arbrume.generate`` as ``surroundings``. Only species in the self-organizing mode
read them, so nothing is voxelized for a parametric species.

A voxelized object is cached by a hash of its evaluated geometry, its transform relative
to the plant and the voxel settings, so dragging a slider of the plant voxelizes nothing
again; moving the object or the plant, or editing the mesh, does.
"""

from __future__ import annotations

import time
from collections import OrderedDict
from dataclasses import dataclass, field

import bpy
import numpy as np

import arbrume

from .logic import voxels

CACHE_SIZE = 32
"""Voxelized objects kept in the session."""
_CACHE: OrderedDict[str, arbrume.VoxelMask] = OrderedDict()


@dataclass
class Report:
    """What building the surroundings of a plant took."""

    milliseconds: float = 0.0
    voxelized: int = 0
    cached: int = 0
    filled: int = 0
    cell: float = 0.0
    errors: list[str] = field(default_factory=list)


def clear_cache() -> None:
    """Forgets every voxelized object."""
    _CACHE.clear()


def is_volume(obj: bpy.types.Object | None) -> bool:
    """Whether ``obj`` can be an envelope or an obstacle: a mesh object that is no plant."""
    if obj is None or obj.type != "MESH":
        return False
    settings = getattr(obj, "arbrume", None)
    return settings is None or not settings.is_plant


def mesh_triangles(obj: bpy.types.Object, depsgraph) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Vertex coordinates, triangle corners and world matrix of the evaluated object."""
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        mesh.calc_loop_triangles()
        coords = np.empty(3 * len(mesh.vertices), dtype=np.float32)
        mesh.vertices.foreach_get("co", coords)
        corners = np.empty(3 * len(mesh.loop_triangles), dtype=np.int32)
        mesh.loop_triangles.foreach_get("vertices", corners)
    finally:
        evaluated.to_mesh_clear()
    matrix = np.array(evaluated.matrix_world, dtype=np.float64)
    return coords.reshape(-1, 3), corners.reshape(-1, 3), matrix


def voxel_mask(plant_obj, obj, cell: float, ground: bool, report: Report) -> arbrume.VoxelMask:
    """The voxel mask of ``obj`` in the frame of ``plant_obj``, from the cache when its
    geometry, transform and settings are unchanged."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    coords, corners, matrix = mesh_triangles(obj, depsgraph)
    relative = voxels.blender_to_plant(np.array(plant_obj.matrix_world), matrix)
    key = voxels.geometry_key(coords, corners, relative, cell, ground)
    mask = _CACHE.get(key)
    if mask is not None:
        _CACHE.move_to_end(key)
        report.cached += 1
    else:
        points = voxels.transform(coords, relative)
        grid = voxels.voxelize(points[corners], cell)
        if ground:
            grid = voxels.with_ground_column(grid)
        mask = arbrume.voxel_mask(grid.origin, grid.cell, grid.shape, grid.mask)
        _CACHE[key] = mask
        while len(_CACHE) > CACHE_SIZE:
            _CACHE.popitem(last=False)
        report.voxelized += 1
    report.filled += mask.filled
    return mask


def cell_size(settings, species: dict) -> float:
    """The voxel edge of a plant: its own setting, or derived from its species and scale."""
    if settings.voxel_size > 0.0:
        return float(settings.voxel_size)
    return voxels.auto_cell(species, float(settings.scale))


def build(plant_obj, species: dict) -> tuple[dict | None, Report]:
    """The ``surroundings`` argument of a plant and what building it took; ``None`` when
    the plant names no volume or its species ignores them."""
    settings = plant_obj.arbrume
    report = Report()
    obstacles = [item for item in settings.obstacles if is_volume(item.obj)]
    envelope = settings.envelope if is_volume(settings.envelope) else None
    if not voxels.is_self_organizing(species) or (envelope is None and not obstacles):
        return None, report
    start = time.perf_counter()
    report.cell = cell = cell_size(settings, species)
    surroundings: dict = {"occluders": []}
    errors = (ValueError, TypeError, RuntimeError)
    if envelope is not None:
        try:
            ground = bool(settings.envelope_ground)
            surroundings["envelope"] = voxel_mask(plant_obj, envelope, cell, ground, report)
        except errors as error:
            report.errors.append(f"{envelope.name}: {error}")
    for item in obstacles:
        try:
            mask = voxel_mask(plant_obj, item.obj, cell, False, report)
        except errors as error:
            report.errors.append(f"{item.obj.name}: {error}")
            continue
        occluder = {"shape": mask, "blocks": bool(item.blocks), "shade": float(item.shade)}
        surroundings["occluders"].append(occluder)
    report.milliseconds = (time.perf_counter() - start) * 1e3
    return surroundings, report
