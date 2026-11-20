"""glb output: structure, and byte equality with the command line when it is built.

Set ``ARBRUME_CLI`` to the path of the ``arbrume`` binary to compare with it.
"""

import json
import os
import struct
import subprocess
from pathlib import Path

import arbrume
import pytest

GLB_MAGIC = 0x46546C67
CHUNK_JSON = 0x4E4F534A


def parse_glb(data: bytes) -> dict:
    """The JSON chunk of a glb file, after checking the header and chunk lengths."""
    magic, version, length = struct.unpack_from("<III", data, 0)
    assert (magic, version, length) == (GLB_MAGIC, 2, len(data))
    json_length, kind = struct.unpack_from("<II", data, 12)
    assert kind == CHUNK_JSON
    document = json.loads(data[20 : 20 + json_length])
    offset = 20 + json_length
    if offset < len(data):
        bin_length, _ = struct.unpack_from("<II", data, offset)
        assert offset + 8 + bin_length == len(data)
        assert document["buffers"][0]["byteLength"] == bin_length
    return document


def test_glb_structure(tree: arbrume.Tree) -> None:
    document = parse_glb(tree.to_glb())
    primitives = document["meshes"][0]["primitives"]
    assert len(primitives) == len(tree.parts)
    for primitive, part in zip(primitives, tree.parts, strict=True):
        count = document["accessors"][primitive["attributes"]["POSITION"]]["count"]
        assert count == part.vertex_count
    extras = document["asset"]["extras"]
    assert extras == tree.metadata
    assert document["nodes"][0]["name"] == "tree"


def test_extras_are_merged(tree: arbrume.Tree) -> None:
    document = parse_glb(tree.to_glb(extras={"label": "x", "index": 3}))
    assert document["asset"]["extras"]["label"] == "x"
    assert document["asset"]["extras"]["seed"] == tree.seed


@pytest.mark.skipif("ARBRUME_CLI" not in os.environ, reason="ARBRUME_CLI is not set")
def test_glb_equals_the_command_line(tmp_path: Path) -> None:
    base_seed, age, quality = 5, 6, 0.5
    command = [os.environ["ARBRUME_CLI"], "gen", "--species", "shrub", "--count", "2"]
    command += ["--seed", str(base_seed), "--age", str(age), "--quality", str(quality)]
    command += ["--textures", "none"]
    subprocess.run([*command, "--out", str(tmp_path), "--quiet"], check=True)
    for index in range(2):
        seed = arbrume.plant_seed(base_seed, index)
        tree = arbrume.generate("shrub", seed=seed, age=age, quality=quality)
        extras = {
            "label": "shrub",
            "base_seed": base_seed,
            "index": index,
            "lod": 0,
            "quality": quality,
        }
        name = f"shrub_{base_seed}_{index}"
        expected = (tmp_path / f"{name}.glb").read_bytes()
        assert tree.to_glb(name=name, extras=extras) == expected
