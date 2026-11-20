"""The forest helpers of the extension, without Blender: mix, grid, holes, frames."""

import arbrume
import numpy as np
import pytest
from logic import forest
from logic.meshdata import y_up_to_z_up


def test_parse_mix() -> None:
    assert forest.parse_mix("shrub:300, conifer:60") == [("shrub", 300.0), ("conifer", 60.0)]
    assert forest.parse_mix("pack/oak") == [("pack/oak", 100.0)]
    with pytest.raises(ValueError, match="negative"):
        forest.parse_mix("shrub:-1")
    with pytest.raises(ValueError, match="empty"):
        forest.parse_mix(" , ")


def test_grid_runs_along_blender_minus_y() -> None:
    grid = forest.grid_over((-10.0, -4.0), (10.0, 6.0), 5.0)
    assert (grid.columns, grid.rows) == (5, 3)
    xs, ys = grid.blender_xy()
    assert xs[0] == -10.0 and ys[0] == 6.0
    assert xs[-1] == 10.0 and ys[-1] == -4.0


def test_holes_are_filled_and_excluded() -> None:
    grid = forest.grid_over((0.0, 0.0), (30.0, 30.0), 10.0)
    heights = np.arange(16, dtype=np.float64)
    heights[[0, 5]] = np.nan
    config = forest.forest_config(grid, heights, [("shrub", 300.0)], {"seed": 3})
    filled = np.array(config["terrain"]["grid"]["heights"]).reshape(4, 4)
    assert np.all(np.isfinite(filled))
    assert filled[0, 0] == 2.5
    assert filled[1, 1] == 5.0
    mask = config["exclude"][0]["mask_grid"]
    assert mask["values"][0] == 1.0 and mask["values"][5] == 1.0 and sum(mask["values"]) == 2.0


def test_rotations_match_the_mesh_frame() -> None:
    rng = np.random.default_rng(1)
    quats = rng.normal(size=(20, 4)).astype(np.float32)
    quats /= np.linalg.norm(quats, axis=1, keepdims=True)
    vector = np.array([[0.3, 1.0, -0.2]], dtype=np.float32)

    def rotate(q, v):
        w, u = q[0], q[1:]
        return v + 2.0 * np.cross(u, np.cross(u, v) + w * v)

    blender = forest.blender_quaternions(quats)
    for arbrume_q, blender_q in zip(quats, blender, strict=True):
        turned = rotate(np.r_[arbrume_q[3], arbrume_q[:3]], vector[0])
        expected = y_up_to_z_up(turned[None, :].astype(np.float32))[0]
        got = rotate(blender_q, y_up_to_z_up(vector)[0])
        assert np.allclose(got, expected, atol=1e-5)


def test_instance_indices_and_placement() -> None:
    indices = forest.instance_indices(np.array([0, 1, 1, 2]), np.array([1, 0, 2, 0]), [2, 3, 1])
    assert indices.tolist() == [1, 2, 4, 5]
    grid = forest.grid_over((-20.0, -20.0), (20.0, 20.0), 4.0)
    heights = np.zeros(grid.columns * grid.rows)
    config = forest.forest_config(grid, heights, [("shrub", 300.0)], {"seed": 3})
    config["species"][0]["spacing"] = 2.0
    placed = arbrume.forest(config, grow=False)
    points = forest.blender_points(placed.positions)
    assert np.all(np.abs(points[:, :2]) <= 20.0)
    assert np.allclose(points[:, 2], 0.0)
