"""Surroundings, voxel masks and growth frames.

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

SPECIES = "envelope_tree"
WALL = {"shape": "box", "min": [0.6, 0.0, -3.0], "max": [0.9, 4.0, 3.0]}
SURROUNDINGS = {
    "envelope": {"shape": "ellipsoid", "center": [0.0, 2.0, 0.0], "radii": [1.5, 2.0, 1.5]},
    "occluders": [WALL, {**WALL, "min": [-4.0, 3.0, -1.0], "blocks": False, "shade": 2.0}],
}
SURROUNDINGS_TOML = """
envelope = { shape = "ellipsoid", center = [0.0, 2.0, 0.0], radii = [1.5, 2.0, 1.5] }

[[occluders]]
shape = "box"
min = [0.6, 0.0, -3.0]
max = [0.9, 4.0, 3.0]

[[occluders]]
shape = "box"
min = [-4.0, 3.0, -1.0]
max = [0.9, 4.0, 3.0]
blocks = false
shade = 2.0
"""


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


def vertices(tree: arbrume.Tree) -> list[np.ndarray]:
    """The vertex array of every part of a tree."""
    return [part.vertices for part in tree.parts]


def ball_mask(center: tuple, radius: float, cell: float) -> arbrume.VoxelMask:
    """A voxel ball, with a column of voxels down to the ground below its centre."""
    low = np.array(center) - radius
    low[1] = -cell
    count = np.ceil((np.array(center) + radius - low) / cell).astype(int)
    axes = [low[i] + (np.arange(count[i]) + 0.5) * cell for i in range(3)]
    x, y, z = np.meshgrid(*axes, indexing="ij")
    inside = (x - center[0]) ** 2 + (y - center[1]) ** 2 + (z - center[2]) ** 2 < radius**2
    inside |= (np.abs(x - center[0]) < cell) & (np.abs(z - center[2]) < cell) & (y < center[1])
    return arbrume.voxel_mask(tuple(low), cell, tuple(count), inside)


def test_dict_file_and_many_agree(tmp_path: Path) -> None:
    path = tmp_path / "surroundings.toml"
    path.write_text(SURROUNDINGS_TOML, encoding="utf-8")
    by_dict = arbrume.generate(SPECIES, seed=3, age=10, surroundings=SURROUNDINGS)
    by_file = arbrume.generate(SPECIES, seed=3, age=10, surroundings=path)
    many = arbrume.generate_many(SPECIES, [3], age=10, surroundings=SURROUNDINGS)
    jobs = arbrume.generate_many(
        [{"species": SPECIES, "seed": 3, "surroundings": str(path)}], age=10
    )
    alone = arbrume.generate(SPECIES, seed=3, age=10)
    assert by_dict.to_glb() == by_file.to_glb() == many[0].to_glb() == jobs[0].to_glb()
    assert by_dict.to_glb() != alone.to_glb()
    assert by_dict.metadata["surroundings"]["occluders"][1]["shade"] == 2.0


@pytest.mark.skipif("ARBRUME_CLI" not in os.environ, reason="ARBRUME_CLI is not set")
def test_dict_equals_the_command_line_file(tmp_path: Path) -> None:
    path = tmp_path / "surroundings.toml"
    path.write_text(SURROUNDINGS_TOML, encoding="utf-8")
    command = [os.environ["ARBRUME_CLI"], "gen", "--species", SPECIES, "--count", "1"]
    command += ["--seed", "4", "--age", "10", "--surroundings", str(path), "--textures", "none"]
    subprocess.run([*command, "--out", str(tmp_path), "--quiet"], check=True)
    written = glb_positions((tmp_path / f"{SPECIES}_4_0.glb").read_bytes())
    seed = arbrume.plant_seed(4, 0)
    tree = arbrume.generate(SPECIES, seed=seed, age=10, surroundings=SURROUNDINGS)
    assert len(written) == len(tree.parts)
    for expected, actual in zip(written, vertices(tree), strict=True):
        assert np.array_equal(expected, actual)


def test_voxel_envelope_keeps_the_plant_inside() -> None:
    mask = ball_mask((0.0, 1.6, 0.0), 0.9, 0.1)
    assert mask.contains((0.0, 1.6, 0.0)) and not mask.contains((0.7, 1.6, 0.7))
    assert mask.shape == (18, 26, 18)
    tree = arbrume.generate(SPECIES, seed=1, age=12, surroundings={"envelope": mask})
    alone = arbrume.generate(SPECIES, seed=1, age=12)
    low, high = (np.array(corner) for corner in mask.bounds)
    reach = float(tree.skeleton.radii.max())
    nodes = tree.skeleton.positions
    assert np.all(nodes >= low - 1e-5) and np.all(nodes <= high + 1e-5)
    bark = tree.parts[0].vertices
    assert np.all(bark >= low - reach) and np.all(bark <= high + reach)
    assert np.ptp(alone.skeleton.positions[:, 1]) > np.ptp(nodes[:, 1])
    assert "voxel mask" in tree.metadata["surroundings"]


def in_wall(points: np.ndarray) -> np.ndarray:
    """Which points lie in the wall of :func:`test_voxel_obstacle_is_avoided`."""
    return (points[:, 0] > 0.4) & (points[:, 0] < 0.7) & (points[:, 1] < 4.0)


def test_voxel_obstacle_is_avoided() -> None:
    cell = 0.1
    wall = np.ones((3, 40, 60), dtype=bool)
    mask = arbrume.voxel_mask((0.4, 0.0, -3.0), cell, (3, 40, 60), wall)
    assert mask.filled == wall.size
    tree = arbrume.generate(
        "open_grown_broadleaf", seed=2, age=14, surroundings={"occluders": [mask]}
    )
    alone = arbrume.generate("open_grown_broadleaf", seed=2, age=14)
    assert in_wall(alone.skeleton.positions).any()
    assert not in_wall(tree.skeleton.positions).any()
    shading = {"occluders": [{"shape": mask, "blocks": False, "shade": 3.0}]}
    shaded = arbrume.generate("open_grown_broadleaf", seed=2, age=14, surroundings=shading)
    assert shaded.to_glb() != alone.to_glb()
    assert in_wall(shaded.skeleton.positions).any()


def test_growth_frames_follow_the_timeline() -> None:
    frames = arbrume.growth_frames("hedge_shrub", seed=2, age=8, frames=3, quality=0.5)
    assert [frame.max_cycle for frame in frames] == [2, 5, 7]
    heights = [frame.stats["height"] for frame in frames]
    assert heights == sorted(heights)
    assert len(arbrume.growth_frames("hedge_shrub", seed=2, age=4, frames=10)) == 4
    assert arbrume.growth_frames("hedge_shrub", seed=2, age=0, frames=3) == []
    whole = arbrume.generate("hedge_shrub", seed=2, age=8, quality=0.5)
    assert whole.max_cycle is None
    assert_same_tree(frames[-1], whole)


def assert_same_tree(frame: arbrume.Tree, whole: arbrume.Tree) -> None:
    """The mesh, skeleton and organ sites of ``frame`` are those of ``whole``, bit for bit."""
    assert frame.stats == whole.stats
    for frame_part, part in zip(frame.parts, whole.parts, strict=True):
        for name in ("vertices", "normals", "uvs", "indices"):
            assert np.array_equal(getattr(frame_part, name), getattr(part, name)), name
    assert np.array_equal(frame.skeleton.positions, whole.skeleton.positions)
    assert np.array_equal(frame.skeleton.radii, whole.skeleton.radii)
    for name in ("nodes", "kinds", "directions", "normals", "scales", "births"):
        assert np.array_equal(getattr(frame.organs, name), getattr(whole.organs, name)), name


@pytest.mark.parametrize("species", ["broadleaf", "conifer", "hedge_shrub", "palm_like"])
def test_the_last_growth_frame_is_the_generated_tree(species: str) -> None:
    frames = arbrume.growth_frames(species, seed=5, age=9, frames=3, quality=0.6)
    assert_same_tree(frames[-1], arbrume.generate(species, seed=5, age=9, quality=0.6))


def test_growth_frames_take_surroundings() -> None:
    frames = arbrume.growth_frames(SPECIES, seed=3, age=10, frames=2, surroundings=SURROUNDINGS)
    whole = arbrume.generate(SPECIES, seed=3, age=10, surroundings=SURROUNDINGS)
    assert_same_tree(frames[-1], whole)
    assert frames[-1].metadata["surroundings"] == whole.metadata["surroundings"]


@pytest.mark.parametrize(
    ("call", "error", "match"),
    [
        (lambda: arbrume.voxel_mask((0, 0, 0), 0.0, (1, 1, 1), [[[True]]]), ValueError, "cell"),
        (lambda: arbrume.voxel_mask((0, 0, 0), 1.0, (2, 1, 1), [[[True]]]), ValueError, "shape"),
        (lambda: arbrume.voxel_mask((0, 0, 0), 1.0, (0, 1, 1), []), ValueError, "voxels"),
        (lambda: arbrume.generate("shrub", age=2, surroundings={"walls": []}), ValueError, "walls"),
        (lambda: arbrume.generate("shrub", age=2, surroundings=3), TypeError, "surroundings"),
        (
            lambda: arbrume.generate("shrub", age=2, surroundings="no/such.toml"),
            FileNotFoundError,
            "such",
        ),
        (
            lambda: arbrume.generate(
                "shrub",
                age=2,
                surroundings={"envelope": {"shape": "sphere", "center": [0, 0, 0], "radius": 0}},
            ),
            ValueError,
            "volume",
        ),
        (lambda: arbrume.growth_frames("shrub", age=2, frames=0), ValueError, "frames"),
    ],
)
def test_errors(call, error: type[Exception], match: str) -> None:
    with pytest.raises(error, match=match):
        call()
