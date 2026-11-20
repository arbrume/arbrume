"""Guides: painted strokes that steer self-organizing growth.

Set ``ARBRUME_CLI`` to the path of the ``arbrume`` binary to compare with it.
"""

import json
import os
import struct
import subprocess
from pathlib import Path

import arbrume
import numpy as np
import pytest

SPECIES = "open_grown_broadleaf"
AGE = 12
LIMB = [[0.0, 2.0, 0.0], [1.0, 2.6, 0.5], [2.0, 3.0, 0.0], [3.0, 3.4, -0.5]]
GUIDES = {
    "strokes": [
        {"kind": "crown", "points": [[3.0, 5.0, 0.0], [4.0, 5.5, 0.0]], "radius": 1.0},
        {"kind": "limb", "points": LIMB, "radius": 0.4, "follow": 0.9},
        {"kind": "erase", "points": [[-2.0, 4.0, 0.0]], "radius": 1.0},
    ]
}


def glb_positions(data: bytes) -> list[np.ndarray]:
    """The POSITION array of every primitive of the first mesh of a glb file."""
    json_length = struct.unpack_from("<I", data, 12)[0]
    document = json.loads(data[20 : 20 + json_length])
    binary = data[20 + json_length + 8 :]
    arrays = []
    for primitive in document["meshes"][0]["primitives"]:
        accessor = document["accessors"][primitive["attributes"]["POSITION"]]
        view = document["bufferViews"][accessor["bufferView"]]
        start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        values = np.frombuffer(binary, "<f4", 3 * accessor["count"], start)
        arrays.append(values.reshape(-1, 3))
    return arrays


def positions(tree: arbrume.Tree) -> np.ndarray:
    """Skeleton positions of a tree."""
    return np.asarray(tree.skeleton.positions)


def test_guides_change_the_plant_and_report_each_stroke():
    plain = arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3)
    guided = arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3, guides=GUIDES)
    assert plain.guides is None
    assert not np.array_equal(positions(plain), positions(guided))
    report = guided.guides["strokes"]
    assert len(report) == 3
    assert report[0]["markers"] > 0
    assert report[1]["claimed"] and report[1]["length"] > 3.0
    assert guided.metadata["guides"]["strokes"][1]["kind"] == "limb"
    assert "guides" not in plain.metadata


def test_dict_text_and_file_give_the_same_plant(tmp_path: Path):
    path = tmp_path / "guides.json"
    path.write_text(json.dumps(GUIDES))
    trees = [
        arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3, guides=value)
        for value in (GUIDES, json.dumps(GUIDES), str(path), path)
    ]
    for tree in trees[1:]:
        assert np.array_equal(positions(tree), positions(trees[0]))


def test_empty_guides_and_parametric_species_are_unchanged():
    plain = arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3)
    empty = arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3, guides={})
    assert np.array_equal(positions(plain), positions(empty))
    assert empty.guides is None
    shrub = arbrume.generate("shrub", seed=2, age=4)
    steered = arbrume.generate("shrub", seed=2, age=4, guides=GUIDES)
    assert np.array_equal(positions(shrub), positions(steered))


def test_batches_and_growth_frames_take_guides():
    single = arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3, guides=GUIDES)
    batch = arbrume.generate_many(SPECIES, [2, 3], age=AGE, quality=0.3, guides=GUIDES)
    assert np.array_equal(positions(batch[0]), positions(single))
    frames = arbrume.growth_frames(SPECIES, seed=2, age=AGE, frames=3, quality=0.3, guides=GUIDES)
    assert np.array_equal(positions(frames[-1]), positions(single))
    assert frames[-1].guides == single.guides


@pytest.mark.parametrize(
    "guides",
    [
        {"strokes": [{"kind": "crown", "points": [], "radius": 1.0}]},
        {"strokes": [{"kind": "limb", "points": [[0, 0, 0]], "radius": 1.0}]},
        {"strokes": [{"kind": "erase", "points": [[0, 0, 0]], "radius": -1.0}]},
        {"strokes": [{"kind": "paint", "points": [[0, 0, 0]], "radius": 1.0}]},
        {"freezes": [4, 2]},
        "not json",
    ],
)
def test_invalid_guides_are_rejected(guides):
    with pytest.raises((ValueError, FileNotFoundError)):
        arbrume.generate(SPECIES, seed=2, age=4, guides=guides)


def test_a_frozen_plant_keeps_its_skeleton():
    frozen = arbrume.generate(SPECIES, seed=2, age=AGE, quality=0.3)
    later = {
        "strokes": [
            {"kind": "crown", "points": [[2.5, 5.0, 2.0]], "radius": 1.0, "cycle": AGE},
        ],
        "freezes": [AGE],
    }
    grown = arbrume.generate(SPECIES, seed=2, age=AGE + 4, quality=0.3, guides=later)
    old, new = frozen.skeleton, grown.skeleton
    born = np.asarray(new.births) < AGE
    kept = np.asarray(new.positions)[born]
    assert len(kept) == len(np.asarray(old.positions))
    key = np.lexsort(kept.T)
    original = np.asarray(old.positions)
    assert np.array_equal(kept[key], original[np.lexsort(original.T)])
    assert born.sum() < len(born)


@pytest.mark.skipif(not os.environ.get("ARBRUME_CLI"), reason="ARBRUME_CLI is not set")
def test_command_line_reads_a_guides_file(tmp_path: Path):
    path = tmp_path / "guides.json"
    path.write_text(json.dumps(GUIDES))
    command = [os.environ["ARBRUME_CLI"], "gen", "--species", SPECIES, "--count", "1"]
    command += ["--seed", "4", "--age", str(AGE), "--guides", str(path), "--textures", "none"]
    subprocess.run([*command, "--out", str(tmp_path), "--quiet"], check=True)
    written = glb_positions((tmp_path / f"{SPECIES}_4_0.glb").read_bytes())
    tree = arbrume.generate(SPECIES, seed=arbrume.plant_seed(4, 0), age=AGE, guides=GUIDES)
    assert len(written) == len(tree.parts)
    for expected, part in zip(written, tree.parts, strict=True):
        assert np.array_equal(expected, part.vertices)
