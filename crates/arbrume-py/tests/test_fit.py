"""`distance` and `fit`: skeleton inputs, determinism and checkpoints."""

from pathlib import Path

import arbrume
import numpy as np
import pytest

FAST = {
    "params": ["growth.axis_length", "scale"],
    "seeds": 2,
    "budget": 24,
    "holdout": 2,
}


FORK = {
    "position": [[0, 0, 0], [0, 1, 0], [0, 2, 0], [0.7, 1.7, 0]],
    "radius": [0.1, 0.1, 0.08, 0.04],
    "parent": [-1, 0, 1, 1],
}
"""A hand-made skeleton dict: a trunk and one branch."""


def test_distance_of_trees_paths_and_dicts(tree: arbrume.Tree, tmp_path: Path) -> None:
    same = arbrume.distance(tree, tree)
    assert same["total"] == 0.0
    assert set(same) == {"envelope", "density", "geometry", "counts", "features", "total"}
    other = arbrume.generate("shrub", seed=8, age=4)
    terms = arbrume.distance(tree, other)
    assert 0.0 < terms["total"] <= 1.0
    assert arbrume.distance(other, tree) == terms
    fork = FORK
    assert arbrume.distance(fork, fork)["total"] == 0.0
    lines = ["id,x,y,z,radius,parent"]
    for i, (p, r, q) in enumerate(
        zip(fork["position"], fork["radius"], fork["parent"], strict=True)
    ):
        lines.append(f"{i},{p[0]},{p[1]},{p[2]},{r},{q}")
    path = tmp_path / "fork.csv"
    path.write_text("\n".join(lines) + "\n")
    assert arbrume.distance(path, fork)["total"] == 0.0
    arrays = {key: np.asarray(value) for key, value in fork.items()}
    assert arbrume.distance(arrays, fork)["total"] == 0.0


def test_bad_inputs_raise(tree: arbrume.Tree) -> None:
    with pytest.raises(TypeError):
        arbrume.distance(tree, 3)
    with pytest.raises(ValueError):
        arbrume.distance(tree, {"position": [[0, 0, 0]], "radius": [0.1], "parent": [5]})
    with pytest.raises(TypeError):
        arbrume.fit(tree, "shrub", budgett=3)


def test_fit_is_deterministic_and_resumes(tree: arbrume.Tree, tmp_path: Path) -> None:
    one = arbrume.fit(tree, "shrub", threads=1, **FAST)
    many = arbrume.fit(tree, "shrub", threads=3, **FAST)
    assert one["toml"] == many["toml"]
    assert one["report"]["after"] == many["report"]["after"]
    assert set(one["factors"]) == {"growth.axis_length"}
    assert one["report"]["after"]["train"]["total"] <= one["report"]["start"]["train"]["total"]
    checkpoint = tmp_path / "fit.checkpoint.json"
    first = arbrume.fit(tree, "shrub", checkpoint=checkpoint, **FAST)
    assert checkpoint.exists()
    resumed = arbrume.fit(tree, "shrub", checkpoint=checkpoint, resume=True, **FAST)
    assert resumed["toml"] == first["toml"] == one["toml"]
    species = tmp_path / "fitted.toml"
    species.write_text(one["toml"])
    grown = arbrume.generate(species, seed=1, age=one["age"], scale=one["scale"])
    assert len(grown.skeleton) > 0
