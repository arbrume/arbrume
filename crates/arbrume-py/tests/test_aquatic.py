"""Aquatic presets through the Python API: water depth, current and surroundings."""

import arbrume
import numpy as np
import pytest

AQUATIC = (
    "kelp_like",
    "bull_kelp_like",
    "seagrass_like",
    "ribbon_grass_like",
    "waterweed_like",
    "water_lily_like",
)


def _top(tree: arbrume.Tree) -> float:
    """Height of the highest node of the skeleton of `tree`."""
    return float(tree.skeleton.positions[:, 1].max())


@pytest.mark.parametrize("species", AQUATIC)
def test_aquatic_presets_are_listed_and_generate(species: str) -> None:
    assert species in arbrume.presets()
    tree = arbrume.generate(species, seed=2, age=8, quality=0.4)
    assert tree.stats["triangles"] > 0


def test_water_depth_sets_the_surface() -> None:
    shallow = arbrume.generate("kelp_like", seed=1, age=10, quality=0.3, water=4.0)
    deep = arbrume.generate("kelp_like", seed=1, age=10, quality=0.3, water=8.0)
    assert _top(shallow) <= 4.0 + 1e-3
    assert 4.5 < _top(deep) <= 8.0 + 1e-3


def test_water_dict_and_surroundings_key_agree() -> None:
    water = {"surface": 6.0, "current": [0.3, 0.0, 0.0], "attenuation": 0.1}
    by_keyword = arbrume.generate("bull_kelp_like", seed=3, age=10, quality=0.3, water=water)
    by_surroundings = arbrume.generate(
        "bull_kelp_like", seed=3, age=10, quality=0.3, surroundings={"water": water}
    )
    assert np.array_equal(by_keyword.parts[0].vertices, by_surroundings.parts[0].vertices)
    still = arbrume.generate("bull_kelp_like", seed=3, age=10, quality=0.3, water=6.0)
    drift = by_keyword.skeleton.positions[:, 0].mean() - still.skeleton.positions[:, 0].mean()
    assert drift > 0.3


def test_bad_water_is_an_error() -> None:
    with pytest.raises(ValueError, match="attenuation"):
        arbrume.generate("kelp_like", seed=1, age=4, water={"attenuation": -1.0})
    with pytest.raises(TypeError, match="water"):
        arbrume.generate("kelp_like", seed=1, age=4, water="deep")
    with pytest.raises(ValueError, match="unknown key"):
        arbrume.generate("kelp_like", seed=1, age=4, surroundings={"waters": 3.0})


def test_land_plants_ignore_water() -> None:
    dry = arbrume.generate("shrub", seed=5, age=4, quality=0.5)
    wet = arbrume.generate("shrub", seed=5, age=4, quality=0.5, water=1.0)
    assert np.array_equal(dry.parts[0].vertices, wet.parts[0].vertices)
