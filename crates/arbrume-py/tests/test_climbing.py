"""Climbing presets on supports given as analytic shapes and as voxel masks."""

import arbrume
import numpy as np

WALL_FACE = -0.12
WALL = {"shape": "box", "min": [-3.0, 0.0, -0.42], "max": [3.0, 4.0, WALL_FACE]}


def wall_mask(cell: float) -> arbrume.VoxelMask:
    """The wall as a voxel mask, whose face lies on a voxel boundary."""
    low = np.array([-3.0, 0.0, -0.42])
    count = (round(6.0 / cell), round(4.0 / cell), round(0.3 / cell))
    inside = np.ones(count, dtype=bool)
    return arbrume.voxel_mask(tuple(low), cell, count, inside)


def bark(tree: arbrume.Tree) -> np.ndarray:
    """The vertices of the bark part."""
    return next(part.vertices for part in tree.parts if part.kind == "bark")


def test_ivy_climbs_a_box_and_a_voxel_wall() -> None:
    for wall in (WALL, wall_mask(0.05)):
        tree = arbrume.generate("ivy_like", seed=4, age=10, surroundings={"occluders": [wall]})
        vertices = bark(tree)
        assert vertices[:, 1].max() > 1.5
        assert vertices[:, 2].min() > WALL_FACE - 1e-3


def test_climbers_are_deterministic_and_creep_without_support() -> None:
    surroundings = {"occluders": [WALL]}
    first = arbrume.generate("vine_like", seed=2, age=8, surroundings=surroundings)
    again = arbrume.generate("vine_like", seed=2, age=8, surroundings=surroundings)
    assert np.array_equal(bark(first), bark(again))
    creeper = arbrume.generate("creeper_like", seed=2, age=8)
    assert bark(creeper)[:, 1].max() < 0.1


def test_the_climbing_table_overrides_like_any_other() -> None:
    surroundings = {"occluders": [WALL]}
    held = arbrume.generate("ivy_like", seed=1, age=8, surroundings=surroundings)
    loose = arbrume.generate(
        "ivy_like", seed=1, age=8, surroundings=surroundings, **{"climbing.gap": 0.03}
    )
    assert not np.array_equal(bark(held), bark(loose))
