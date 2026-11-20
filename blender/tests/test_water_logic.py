"""The water helpers of the extension, without Blender: surface, current, forest level."""

import arbrume
import numpy as np
from logic import forest, water
from logic.meshdata import y_up_to_z_up


def test_the_surface_comes_from_the_species_a_height_or_an_object() -> None:
    assert water.water_argument("SPECIES", 3.0, None, 0.0, (0.0, 0.0)) is None
    assert water.water_argument("HEIGHT", 3.0, None, 1.0, (0.0, 0.0)) == {
        "current": [0.0, 0.0, -0.0],
        "surface": 3.0,
    }
    by_object = water.water_argument("OBJECT", 0.0, 5.5, 1.5, (0.0, 0.0))
    assert by_object is not None and by_object["surface"] == 4.0
    assert water.water_argument("OBJECT", 0.0, None, 1.5, (0.0, 0.0)) is None
    current = water.water_argument("SPECIES", 0.0, None, 0.0, (0.2, 0.0))
    assert current == {"current": [0.2, 0.0, -0.0]}


def test_the_current_turns_like_a_vector_of_blender() -> None:
    arbrume_vector = np.array([water.current_vector((0.3, -0.4))], dtype=np.float32)
    blender_vector = y_up_to_z_up(arbrume_vector)[0]
    assert np.allclose(blender_vector, [0.3, -0.4, 0.0])


def test_a_water_level_reaches_the_forest_config() -> None:
    assert water.water_level("NONE", 2.0, None) is None
    assert water.water_level("HEIGHT", 2.0, None) == 2.0
    assert water.water_level("OBJECT", 0.0, -1.0) == -1.0
    grid = forest.grid_over((-10.0, -10.0), (10.0, 10.0), 5.0)
    heights = np.linspace(-6.0, 1.0, grid.rows * grid.columns)
    options = {"water_level": 0.0, "variants": 1, "lods": 1, "billboard": "none"}
    config = forest.forest_config(grid, heights, [("bull_kelp_like", 300.0)], options)
    assert config["placement"] == {"water_level": 0.0}
    config["species"][0]["spacing"] = 2.0
    placed = arbrume.forest(config, grow=False)
    assert len(placed.positions) > 0
    assert (placed.positions[:, 1] < 0.0).all()


def test_generate_accepts_the_water_argument() -> None:
    argument = water.water_argument("HEIGHT", 3.0, None, 0.0, (0.0, 0.1))
    tree = arbrume.generate("bull_kelp_like", seed=1, age=8, quality=0.2, water=argument)
    assert tree.skeleton.positions[:, 1].max() <= 3.0 + 1e-3
