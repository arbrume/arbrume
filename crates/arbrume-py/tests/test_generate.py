"""Species arguments, overrides, parameters, errors and metadata of `generate`."""

from pathlib import Path

import arbrume
import pytest


def test_module_metadata() -> None:
    assert arbrume.__version__.count(".") == 2
    names = arbrume.presets()
    assert names == sorted(names)
    assert "shrub" in names
    assert "[" in arbrume.preset_toml("shrub")
    with pytest.raises(ValueError, match="no preset"):
        arbrume.preset_toml("nope")


def test_species_from_preset_file_and_text(tmp_path: Path) -> None:
    text = arbrume.preset_toml("shrub")
    path = tmp_path / "my_shrub.toml"
    path.write_text(text, encoding="utf-8")
    reference = arbrume.generate("shrub", seed=2, age=3).to_glb()
    assert arbrume.generate(path, seed=2, age=3).to_glb() == reference
    assert arbrume.generate(str(path), seed=2, age=3).to_glb() == reference
    assert arbrume.generate(text, seed=2, age=3).to_glb() == reference


def test_parameters_reach_the_tree(tree: arbrume.Tree) -> None:
    assert (tree.seed, tree.age, tree.scale) == (7, 4, 1.0)
    big = arbrume.generate("shrub", seed=7, age=4, scale=2.0)
    assert big.stats["height"] > 1.5 * tree.stats["height"]
    coarse = arbrume.generate("shrub", seed=7, age=4, quality=0.25)
    assert coarse.stats["triangles"] < tree.stats["triangles"]
    meta = tree.metadata
    assert meta["seed"] == 7
    assert meta["age"] == 4
    assert meta["species_name"] == tree.species_name
    assert meta["stats"] == tree.stats


def test_overrides_keyword_and_dict_agree() -> None:
    by_keyword = arbrume.generate("shrub", seed=1, age=3, **{"foliage.length": 0.2})
    by_dict = arbrume.generate("shrub", seed=1, age=3, overrides={"foliage.length": 0.2})
    nested = arbrume.generate("shrub", seed=1, age=3, foliage={"length": 0.2})
    plain = arbrume.generate("shrub", seed=1, age=3)
    assert by_keyword.to_glb() == by_dict.to_glb() == nested.to_glb()
    assert by_keyword.metadata["species"]["foliage"]["length"] == pytest.approx(0.2)
    assert by_keyword.to_glb() != plain.to_glb()


@pytest.mark.parametrize(
    ("kwargs", "error", "match"),
    [
        ({"species": "nope"}, ValueError, "neither a preset"),
        ({"species": "missing/dir/oak.toml"}, FileNotFoundError, "oak.toml"),
        ({"species": "name = 3"}, ValueError, None),
        ({"species": "shrub", "growth.gnarll": 3.0}, ValueError, "gnarll"),
        ({"species": "shrub", "foliage.width_ratio": -1.0}, ValueError, None),
        ({"species": "shrub", "name.x": 1}, ValueError, "cannot override"),
        ({"species": "shrub", "scale": -1.0}, ValueError, "scale"),
        ({"species": "shrub", "quality": 0.0}, ValueError, "quality"),
        ({"species": "shrub", "seed": -1}, ValueError, "seed"),
        ({"species": "shrub", "age": 70000}, ValueError, "age"),
        ({"species": 3.5}, TypeError, "species"),
    ],
)
def test_errors(kwargs: dict, error: type[Exception], match: str | None) -> None:
    with pytest.raises(error, match=match):
        arbrume.generate(**kwargs)


def test_save_glb_names_the_node_after_the_file(tmp_path: Path, tree: arbrume.Tree) -> None:
    path = tmp_path / "bush.glb"
    tree.save_glb(path)
    assert path.read_bytes() == tree.to_glb(name="bush")
    assert tree.to_glb(name="bush") != tree.to_glb()
