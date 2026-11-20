"""generate_stand: trees grown together, in input order, independent of order and threads."""

import arbrume
import numpy as np
import pytest

PLANTS = [
    {"species": "hedge_shrub", "seed": 1, "age": 5, "position": [0.0, 0.0, 0.0]},
    {"species": "open_grown_broadleaf", "seed": 2, "age": 7, "position": [1.0, 0.0, 0.5]},
    {"species": "shrub", "seed": 3, "age": 4, "position": [-1.5, 0.0, 0.0]},
]


def vertices(tree: arbrume.Tree) -> bytes:
    """Bytes of every vertex position of `tree`."""
    return b"".join(np.asarray(part.vertices).tobytes() for part in tree.parts)


def test_a_stand_returns_one_tree_per_plant_in_order() -> None:
    trees = arbrume.generate_stand(PLANTS, quality=0.3)
    assert len(trees) == len(PLANTS)
    alone = arbrume.generate("shrub", seed=3, age=4, quality=0.3)
    assert vertices(trees[2]) == vertices(alone)
    shrub = arbrume.generate("hedge_shrub", seed=1, age=5, quality=0.3)
    assert vertices(trees[0]) != vertices(shrub)


def test_order_and_threads_do_not_matter() -> None:
    want = [vertices(tree) for tree in arbrume.generate_stand(PLANTS, quality=0.3)]
    reversed_trees = arbrume.generate_stand(PLANTS[::-1], quality=0.3, threads=1)
    assert [vertices(tree) for tree in reversed_trees[::-1]] == want


def test_one_plant_at_the_origin_is_the_tree_generated_alone() -> None:
    plant = {"species": "open_grown_broadleaf", "seed": 4, "age": 6, "position": (0, 0, 0)}
    (tree,) = arbrume.generate_stand([plant], quality=0.3)
    alone = arbrume.generate("open_grown_broadleaf", seed=4, age=6, quality=0.3)
    assert vertices(tree) == vertices(alone)


def test_bad_plants_are_rejected() -> None:
    with pytest.raises(ValueError, match="position"):
        arbrume.generate_stand([{"species": "shrub"}])
    with pytest.raises(ValueError, match="3 numbers"):
        arbrume.generate_stand([{"species": "shrub", "position": [0.0, 1.0]}])
    with pytest.raises(TypeError):
        arbrume.generate_stand(["shrub"])
    assert arbrume.generate_stand([]) == []
