"""arbrume.forest: placement, determinism, the library and the written files."""

import json

import arbrume
import numpy as np
import pytest

PLACEMENT = {
    "seed": 3,
    "terrain": {
        "size": [120.0, 80.0],
        "grid": {"columns": 2, "rows": 2, "heights": [0.0, 12.0, 0.0, 12.0]},
    },
    "species": [
        {"species": "shrub", "density": 300.0, "spacing": 2.5, "variants": 3},
        {"species": "conifer", "density": 80.0, "spacing": 4.0, "lean": 3.0},
    ],
    "exclude": [{"circle": {"center": [0.0, 0.0], "radius": 15.0}}],
}

GROWN = {
    "name": "small",
    "terrain": {"size": [30.0, 30.0]},
    "species": [{"species": "shrub", "density": 200.0, "age": 4, "variants": 2}],
    "library": {"lods": 2, "billboard": "none", "textures": False},
    "output": {"tables": ["csv", "bin"]},
}


def test_placement_only() -> None:
    placed = arbrume.forest(PLACEMENT, grow=False)
    n = len(placed.instances)
    assert n > 100
    assert placed.positions.shape == (n, 3)
    assert placed.rotations.shape == (n, 4)
    assert placed.instances.dtype == arbrume.INSTANCE_DTYPE
    assert placed.names == ["shrub", "conifer"]
    assert placed.library == []
    assert np.allclose(np.linalg.norm(placed.rotations, axis=1), 1.0, atol=1e-5)
    x, y, z = placed.positions.T
    assert np.all(np.hypot(x, z) > 15.0)
    assert np.allclose(y, (x + 60.0) / 120.0 * 12.0, atol=1e-3)
    assert placed.terrain["heights"].shape == (2, 2)
    assert set(np.unique(placed.species)) == {0, 1}
    assert placed.variants[placed.species == 0].max() < 3


def test_placement_is_deterministic() -> None:
    one = arbrume.forest(PLACEMENT, grow=False, threads=1)
    four = arbrume.forest(PLACEMENT, grow=False, threads=4)
    assert one.instances.tobytes() == four.instances.tobytes()
    reseeded = arbrume.forest({**PLACEMENT, "seed": 4}, grow=False)
    assert reseeded.instances.tobytes() != one.instances.tobytes()


def test_toml_text_and_errors() -> None:
    text = """
    [terrain]
    size = [40.0, 40.0]
    [[species]]
    species = "shrub"
    density = 300.0
    spacing = 2.0
    """
    assert len(arbrume.forest(text, grow=False).instances) > 10
    with pytest.raises(ValueError, match="spacing"):
        arbrume.forest({**PLACEMENT, "species": [{"species": "shrub", "density": 1.0}]}, grow=False)
    with pytest.raises(ValueError):
        arbrume.forest({"terrain": {"size": [1.0, 1.0]}, "species": [], "bogus": 1}, grow=False)


def test_grown_library_and_files(tmp_path) -> None:
    grown = arbrume.forest(GROWN, out=tmp_path)
    assert len(grown.library) == 1
    variants = grown.library[0]
    assert len(variants) == 2
    for variant in variants:
        assert variant["kinds"] == ["mesh", "mesh"]
        levels = variant["levels"]
        assert all(isinstance(level, arbrume.Tree) for level in levels)
        assert levels[1].stats["triangles"] < levels[0].stats["triangles"]
        assert variant["crown_radius"] > 0.0
    assert grown.variants.max() < 2
    manifest = json.loads((tmp_path / "small.json").read_text())
    assert manifest["trees"] == len(grown.instances)
    names = {entry["file"] for entry in manifest["files"]}
    assert {"shrub_v0.glb", "shrub_v1.glb", "small_instanced.glb", "small_instances.bin"} <= names
    records = (tmp_path / "small_instances.bin").read_bytes()
    table = np.frombuffer(records, dtype=arbrume.INSTANCE_DTYPE, offset=32)
    assert table.tobytes() == grown.instances.tobytes()
