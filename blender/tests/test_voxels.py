"""Voxelizing meshes for surroundings, and the keys of growth frames, without Blender."""

import arbrume
import numpy as np
import pytest
from logic import frames, voxels


def uv_sphere(center, radius: float, rings: int = 16, sides: int = 24) -> np.ndarray:
    """Triangles ``(n, 3, 3)`` of a closed UV sphere."""
    theta = np.linspace(0.0, np.pi, rings + 1)
    phi = np.linspace(0.0, 2.0 * np.pi, sides + 1)
    t, p = np.meshgrid(theta, phi, indexing="ij")
    grid = np.stack([np.sin(t) * np.cos(p), np.cos(t), np.sin(t) * np.sin(p)], axis=-1)
    grid = grid * radius + np.asarray(center)
    a, b = grid[:-1, :-1], grid[1:, :-1]
    c, d = grid[1:, 1:], grid[:-1, 1:]
    tris = np.concatenate([np.stack([a, b, c], -2), np.stack([a, c, d], -2)])
    return tris.reshape(-1, 3, 3)


def box(low, high) -> np.ndarray:
    """Triangles of a closed axis-aligned box."""
    low, high = np.asarray(low, float), np.asarray(high, float)
    corner = [np.where([i & 1, i & 2, i & 4], high, low) for i in range(8)]
    faces = [(0, 2, 3, 1), (4, 5, 7, 6), (0, 1, 5, 4), (2, 6, 7, 3), (0, 4, 6, 2), (1, 3, 7, 5)]
    tris = [[corner[f[0]], corner[f[i]], corner[f[i + 1]]] for f in faces for i in (1, 2)]
    return np.array(tris)


def test_closed_meshes_fill_their_volume() -> None:
    grid = voxels.voxelize(uv_sphere((0.0, 2.0, 0.0), 1.0), 0.05, shell=False)
    volume = grid.filled * grid.cell**3
    assert volume == pytest.approx(4.0 / 3.0 * np.pi * 0.995, rel=0.04)
    cube = voxels.voxelize(box((0.0, 0.0, 0.0), (1.0, 0.5, 0.25)), 0.05, shell=False)
    assert cube.filled == 20 * 10 * 5
    assert cube.mask.shape == cube.shape == (22, 12, 7)
    assert cube.origin == pytest.approx((-0.05, -0.05, -0.05))


def test_the_shell_of_an_open_plane() -> None:
    plane = np.array([[[0, 0, -1], [0, 2, -1], [0, 2, 1]], [[0, 0, -1], [0, 2, 1], [0, 0, 1]]])
    grid = voxels.voxelize(plane + 0.01, 0.1)
    assert grid.shape[0] == 3
    assert grid.mask[1].mean() > 0.8
    assert not grid.mask[0].any() and not grid.mask[2].any()


def test_the_grid_is_limited() -> None:
    grid = voxels.voxelize(box((0, 0, 0), (100, 100, 100)), 0.01, shell=False)
    assert np.prod(grid.shape) <= voxels.MAX_VOXELS
    assert grid.cell > 0.4


def test_ground_column_reaches_the_floating_crown() -> None:
    grid = voxels.voxelize(uv_sphere((0.0, 3.0, 0.0), 1.0), 0.1)
    grounded = voxels.with_ground_column(grid)
    assert grounded.origin[1] < 0.0
    low = np.array(grounded.origin)
    index = np.floor((np.array([0.0, 0.05, 0.0]) - low) / grounded.cell).astype(int)
    assert grounded.mask[tuple(index)]
    assert grounded.filled - grid.filled < 9 * 25
    aside = voxels.voxelize(uv_sphere((5.0, 3.0, 0.0), 1.0), 0.1)
    assert voxels.with_ground_column(aside) is aside


def test_transform_into_the_plant_frame() -> None:
    plant = np.eye(4)
    plant[:3, 3] = (1.0, 2.0, 0.0)
    obj = np.eye(4)
    obj[:3, 3] = (1.0, 5.0, 3.0)
    matrix = voxels.blender_to_plant(plant, obj)
    point = voxels.transform(np.array([[0.0, 0.0, 0.0]]), matrix)
    assert point[0] == pytest.approx((0.0, 3.0, -3.0))
    key = voxels.geometry_key(np.zeros((3, 3)), np.zeros((1, 3), int), matrix, 0.1)
    assert key != voxels.geometry_key(np.zeros((3, 3)), np.zeros((1, 3), int), matrix, 0.2)


def test_cell_from_the_species() -> None:
    growth = {"internode_length": {"by_order": [0.25, [[0.0, 0.3], [1.0, 0.12]], 0.2]}}
    assert voxels.internode_length(growth) == pytest.approx(0.12)
    assert voxels.internode_length({"internode_length": 0.4}) == 0.4
    assert voxels.internode_length({}) is None
    species = arbrume.generate("hedge_shrub", age=0).metadata["species"]
    assert voxels.auto_cell(species, 2.0) == pytest.approx(0.06)
    assert voxels.is_self_organizing(species)
    assert not voxels.is_self_organizing(arbrume.generate("shrub", age=0).metadata["species"])


def test_a_voxelized_envelope_keeps_the_plant_inside() -> None:
    grid = voxels.voxelize(uv_sphere((0.0, 2.2, 0.0), 1.2), 0.1)
    mask = arbrume.voxel_mask(grid.origin, grid.cell, grid.shape, grid.mask)
    tree = arbrume.generate("envelope_tree", seed=1, age=12, surroundings={"envelope": mask})
    nodes = tree.skeleton.positions
    radius = np.linalg.norm(nodes - (0.0, 2.2, 0.0), axis=1)
    crown = nodes[:, 1] > 1.2
    assert crown.sum() > 10
    assert radius[crown].max() < 1.2 + grid.cell * np.sqrt(3)


def test_visibility_keys_show_one_stage_at_a_time() -> None:
    count, start, step = 4, 1, 5
    keys = frames.visibility_keys(count, start, step)
    for frame in range(start - 2, frames.last_frame(count, start, step) + 10):
        shown = []
        for index, stage in enumerate(keys):
            hidden = [h for f, h in stage if f <= frame]
            if not (hidden[-1] if hidden else stage[0][1]):
                shown.append(index)
        assert shown == [frames.visible_stage(count, start, step, frame)], frame
    assert frames.visibility_keys(1, 1, 5) == [[(1, False)]]
